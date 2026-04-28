//! Transaction Monitor - Entry Price Extraction After Buy Confirmation
//!
//! This module monitors confirmed transactions and extracts the real entry price
//! by parsing token balance deltas plus bonding-curve SOL spend from transaction metadata.
//!
//! This closes the information feedback loop: Transaction confirmed → Price Extraction → Revolver Loading
//!
//! # Usage
//!
//! ```ignore
//! let monitor = TransactionMonitor::new(rpc_client);
//! let metadata = monitor.fetch_transaction_metadata(&signature).await?;
//! let entry_price = monitor.extract_entry_price(&metadata, &payer_pubkey)?;
//!
//! // Load magazine with real entry price
//! revolver_worker.load_magazine_from_direct_buy(mint, tokens_received, entry_price).await?;
//! ```

use crate::errors::{Result, TriggerError};
use serde_json::Value;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::Signature, system_program};
use solana_transaction_status::{
    option_serializer::OptionSerializer, EncodedConfirmedTransactionWithStatusMeta,
    EncodedTransaction, UiInstruction, UiMessage, UiParsedInstruction, UiTransactionEncoding,
    UiTransactionTokenBalance,
};
use std::{collections::HashMap, str::FromStr, sync::Arc};
use tracing::{debug, info, warn};

/// Extracted buy transaction metadata
#[derive(Debug, Clone)]
pub struct BuyTransactionMetadata {
    /// Transaction signature
    pub signature: Signature,
    /// Token mint address
    pub mint: Pubkey,
    /// SOL sent into the bonding curve by the executed BUY path (in lamports),
    /// excluding inline Sender tip, payer-side rent, and network-fee noise.
    pub sol_spent: u64,
    /// Tokens received
    pub tokens_received: u64,
    /// Calculated entry price (lamports per token, scaled by 1e9)
    pub entry_price: u64,
    /// Slot when transaction was confirmed
    pub slot: u64,
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

impl BuyTransactionMetadata {
    /// Calculate entry price from sol_spent and tokens_received
    ///
    /// Entry price = (sol_spent / tokens_received) scaled for precision
    /// Returns price in lamports per token (1e9 scale)
    pub fn calculate_entry_price(sol_spent: u64, tokens_received: u64) -> Result<u64> {
        calculate_price_lamports_per_token(
            sol_spent,
            tokens_received,
            "Cannot calculate entry price: tokens_received is 0",
            "Entry price overflow",
        )
    }
}

/// Extracted sell transaction metadata
#[derive(Debug, Clone)]
pub struct SellTransactionMetadata {
    /// Transaction signature
    pub signature: Signature,
    /// Token mint address
    pub mint: Pubkey,
    /// SOL received by the executed swap path (in lamports), excluding payer-side
    /// network fee and parsed outgoing transfers such as inline tip.
    pub sol_received: u64,
    /// Net payer wallet delta in lamports after the entire SELL transaction.
    pub payer_wallet_net_change: i64,
    /// Sum of parsed outgoing payer-side system transfers (typically inline tip).
    pub payer_outgoing_transfer_lamports: u64,
    /// Network fee charged to the payer for the SELL transaction.
    pub network_fee_lamports: u64,
    /// Tokens sold by the transaction
    pub tokens_sold: u64,
    /// Calculated realized exit price (lamports per token, scaled by 1e9)
    pub exit_price: u64,
    /// Slot when transaction was confirmed
    pub slot: u64,
    /// Primary token account that carried the SELL delta.
    pub token_account: Pubkey,
    /// Token balance on that account immediately before the SELL confirmed.
    pub token_balance_before_sell: u64,
    /// Token balance on that account immediately after the SELL confirmed.
    pub token_balance_after_sell: u64,
    /// Token decimals reported in transaction metadata.
    pub token_decimals: u8,
    /// Token program reported in transaction metadata, when available.
    pub token_program: Option<Pubkey>,
}

impl SellTransactionMetadata {
    /// Calculate realized exit price from exact swap proceeds and tokens sold.
    pub fn calculate_exit_price(sol_received: u64, tokens_sold: u64) -> Result<u64> {
        calculate_price_lamports_per_token(
            sol_received,
            tokens_sold,
            "Cannot calculate exit price: tokens_sold is 0",
            "Exit price overflow",
        )
    }
}

fn calculate_price_lamports_per_token(
    sol_lamports: u64,
    token_amount: u64,
    zero_amount_error: &str,
    overflow_error: &str,
) -> Result<u64> {
    if token_amount == 0 {
        return Err(TriggerError::Other(zero_amount_error.to_string()));
    }

    let scaled_sol = (sol_lamports as u128).saturating_mul(1_000_000_000);
    let price = scaled_sol / (token_amount as u128);

    if price > u64::MAX as u128 {
        return Err(TriggerError::Other(overflow_error.to_string()));
    }

    Ok(price as u64)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedBuyTokenPosition {
    token_account: Pubkey,
    token_balance_after_buy: u64,
    tokens_received: u64,
    token_decimals: u8,
    token_program: Option<Pubkey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedSellTokenPosition {
    token_account: Pubkey,
    token_balance_before_sell: u64,
    token_balance_after_sell: u64,
    tokens_sold: u64,
    token_decimals: u8,
    token_program: Option<Pubkey>,
}

/// Transaction Monitor for extracting entry price after buy confirmation
pub struct TransactionMonitor {
    /// RPC client for fetching transaction metadata
    rpc_client: Arc<RpcClient>,
}

impl TransactionMonitor {
    /// Create a new transaction monitor
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self { rpc_client }
    }

    /// Fetch transaction metadata for a confirmed signature
    ///
    /// This fetches the full transaction with pre/post token balances.
    pub async fn fetch_transaction(
        &self,
        signature: &Signature,
    ) -> Result<EncodedConfirmedTransactionWithStatusMeta> {
        debug!("Fetching transaction metadata for signature: {}", signature);

        let config = solana_client::rpc_config::RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::JsonParsed),
            commitment: Some(solana_sdk::commitment_config::CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        };

        let tx = self
            .rpc_client
            .get_transaction_with_config(signature, config)
            .await
            .map_err(|e| {
                TriggerError::Other(format!("Failed to fetch transaction {}: {}", signature, e))
            })?;

        debug!(
            "Fetched transaction: slot={}, meta_present={}",
            tx.slot,
            tx.transaction.meta.is_some()
        );

        Ok(tx)
    }

    /// Extract buy transaction metadata from a confirmed transaction
    ///
    /// Parses preTokenBalances vs postTokenBalances to calculate:
    /// - SOL spent
    /// - Tokens received
    /// - Real entry price
    pub fn extract_buy_metadata(
        &self,
        tx: &EncodedConfirmedTransactionWithStatusMeta,
        payer: &Pubkey,
        mint: &Pubkey,
        signature: Signature,
    ) -> Result<BuyTransactionMetadata> {
        let meta = tx
            .transaction
            .meta
            .as_ref()
            .ok_or_else(|| TriggerError::Other("Transaction has no metadata".to_string()))?;

        let account_keys = extract_transaction_account_keys(tx)?;
        let payer_index = account_keys
            .iter()
            .position(|key| key == payer)
            .ok_or_else(|| {
                TriggerError::Other(format!(
                    "Confirmed transaction does not contain payer account {}",
                    payer
                ))
            })?;

        // Get pre and post balances (SOL)
        let pre_balances = &meta.pre_balances;
        let post_balances = &meta.post_balances;

        // Payer wallet delta is the coarse fallback only.
        let payer_wallet_delta =
            self.calculate_sol_spent(pre_balances, post_balances, payer_index)?;
        let sol_spent = match self.extract_swap_sol_spent(tx, payer)? {
            Some(swap_sol_spent) => {
                if swap_sol_spent != payer_wallet_delta {
                    info!(
                        payer = %payer,
                        payer_wallet_delta_lamports = payer_wallet_delta,
                        swap_sol_spent_lamports = swap_sol_spent,
                        "Resolved bonding-curve SOL spend separately from payer wallet delta"
                    );
                }
                swap_sol_spent
            }
            None => {
                warn!(
                    payer = %payer,
                    payer_wallet_delta_lamports = payer_wallet_delta,
                    "Parsed bonding-curve transfers were unavailable; falling back to payer wallet delta for SOL spend"
                );
                payer_wallet_delta
            }
        };

        // Get pre and post token balances
        let pre_token_balances = match &meta.pre_token_balances {
            OptionSerializer::Some(balances) => balances.clone(),
            _ => vec![],
        };

        let post_token_balances = match &meta.post_token_balances {
            OptionSerializer::Some(balances) => balances.clone(),
            _ => vec![],
        };

        let token_position = self.resolve_primary_buy_token_position(
            &pre_token_balances,
            &post_token_balances,
            &account_keys,
            payer,
            mint,
        )?;
        let fee_recipient = self.extract_pump_fee_recipient(tx);

        // Calculate entry price
        let entry_price = BuyTransactionMetadata::calculate_entry_price(
            sol_spent,
            token_position.tokens_received,
        )?;

        info!(
            signature = %signature,
            mint = %mint,
            token_account = %token_position.token_account,
            token_balance_after_buy = token_position.token_balance_after_buy,
            tokens_received = token_position.tokens_received,
            token_decimals = token_position.token_decimals,
            token_program = ?token_position.token_program,
            fee_recipient = ?fee_recipient,
            payer_wallet_delta_lamports = payer_wallet_delta,
            sol_spent_lamports = sol_spent,
            entry_price,
            "Extracted buy metadata"
        );

        Ok(BuyTransactionMetadata {
            signature,
            mint: *mint,
            sol_spent,
            tokens_received: token_position.tokens_received,
            entry_price,
            slot: tx.slot,
            token_account: token_position.token_account,
            token_balance_after_buy: token_position.token_balance_after_buy,
            token_decimals: token_position.token_decimals,
            token_program: token_position.token_program,
            fee_recipient,
        })
    }

    /// Extract sell transaction metadata from a confirmed transaction
    ///
    /// Parses pre/post token balances plus payer lamport deltas to calculate:
    /// - exact swap SOL received
    /// - tokens sold
    /// - realized exit price
    pub fn extract_sell_metadata(
        &self,
        tx: &EncodedConfirmedTransactionWithStatusMeta,
        payer: &Pubkey,
        mint: &Pubkey,
        signature: Signature,
    ) -> Result<SellTransactionMetadata> {
        let meta = tx
            .transaction
            .meta
            .as_ref()
            .ok_or_else(|| TriggerError::Other("Transaction has no metadata".to_string()))?;

        let account_keys = extract_transaction_account_keys(tx)?;
        let payer_index = account_keys
            .iter()
            .position(|key| key == payer)
            .ok_or_else(|| {
                TriggerError::Other(format!(
                    "Confirmed transaction does not contain payer account {}",
                    payer
                ))
            })?;

        let pre_balances = &meta.pre_balances;
        let post_balances = &meta.post_balances;
        let payer_wallet_net_change =
            self.calculate_net_sol_change(pre_balances, post_balances, payer_index)?;
        let network_fee_lamports = meta.fee;
        let payer_outgoing_transfer_lamports = self
            .extract_total_outgoing_transfer_lamports_from_payer(tx, payer)?
            .unwrap_or(0);

        let gross_sol_received_i128 = i128::from(payer_wallet_net_change)
            .checked_add(i128::from(network_fee_lamports))
            .and_then(|value| value.checked_add(i128::from(payer_outgoing_transfer_lamports)))
            .ok_or_else(|| {
                TriggerError::Other(format!(
                    "Sell proceeds overflow while reconstructing gross SOL received for payer {}",
                    payer
                ))
            })?;

        if gross_sol_received_i128 <= 0 {
            return Err(TriggerError::Other(format!(
                "Unable to derive positive gross SOL received from confirmed SELL {}: net_change={} fee={} outgoing_transfers={}",
                signature,
                payer_wallet_net_change,
                network_fee_lamports,
                payer_outgoing_transfer_lamports
            )));
        }

        let sol_received = u64::try_from(gross_sol_received_i128).map_err(|error| {
            TriggerError::Other(format!(
                "Confirmed SELL gross proceeds do not fit u64 for {}: {}",
                signature, error
            ))
        })?;

        let pre_token_balances = match &meta.pre_token_balances {
            OptionSerializer::Some(balances) => balances.clone(),
            _ => vec![],
        };

        let post_token_balances = match &meta.post_token_balances {
            OptionSerializer::Some(balances) => balances.clone(),
            _ => vec![],
        };

        let token_position = self.resolve_primary_sell_token_position(
            &pre_token_balances,
            &post_token_balances,
            &account_keys,
            payer,
            mint,
        )?;
        let exit_price = SellTransactionMetadata::calculate_exit_price(
            sol_received,
            token_position.tokens_sold,
        )?;

        info!(
            signature = %signature,
            mint = %mint,
            token_account = %token_position.token_account,
            token_balance_before_sell = token_position.token_balance_before_sell,
            token_balance_after_sell = token_position.token_balance_after_sell,
            tokens_sold = token_position.tokens_sold,
            token_decimals = token_position.token_decimals,
            token_program = ?token_position.token_program,
            payer_wallet_net_change_lamports = payer_wallet_net_change,
            payer_outgoing_transfer_lamports,
            network_fee_lamports,
            sol_received_lamports = sol_received,
            exit_price,
            "Extracted sell metadata"
        );

        Ok(SellTransactionMetadata {
            signature,
            mint: *mint,
            sol_received,
            payer_wallet_net_change,
            payer_outgoing_transfer_lamports,
            network_fee_lamports,
            tokens_sold: token_position.tokens_sold,
            exit_price,
            slot: tx.slot,
            token_account: token_position.token_account,
            token_balance_before_sell: token_position.token_balance_before_sell,
            token_balance_after_sell: token_position.token_balance_after_sell,
            token_decimals: token_position.token_decimals,
            token_program: token_position.token_program,
        })
    }

    /// Calculate payer wallet lamport delta by comparing pre and post balances.
    fn calculate_sol_spent(
        &self,
        pre_balances: &[u64],
        post_balances: &[u64],
        payer_index: usize,
    ) -> Result<u64> {
        let (pre_balance, post_balance) =
            self.balance_pair_for_index(pre_balances, post_balances, payer_index)?;

        // SOL spent = pre_balance - post_balance (should be positive for a buy)
        let sol_spent = pre_balance.saturating_sub(post_balance);

        if sol_spent == 0 {
            warn!("SOL spent is 0 - this may indicate the transaction failed or is not a buy");
        }

        debug!(
            "SOL balance change: {} -> {} (spent: {})",
            pre_balance, post_balance, sol_spent
        );

        Ok(sol_spent)
    }

    fn calculate_net_sol_change(
        &self,
        pre_balances: &[u64],
        post_balances: &[u64],
        payer_index: usize,
    ) -> Result<i64> {
        let (pre_balance, post_balance) =
            self.balance_pair_for_index(pre_balances, post_balances, payer_index)?;
        let net_change = i128::from(post_balance) - i128::from(pre_balance);
        i64::try_from(net_change).map_err(|error| {
            TriggerError::Other(format!(
                "Net lamport change does not fit i64 for payer index {}: {}",
                payer_index, error
            ))
        })
    }

    fn balance_pair_for_index(
        &self,
        pre_balances: &[u64],
        post_balances: &[u64],
        payer_index: usize,
    ) -> Result<(u64, u64)> {
        if payer_index >= pre_balances.len() || payer_index >= post_balances.len() {
            return Err(TriggerError::Other(format!(
                "Payer index {} out of bounds (pre: {}, post: {})",
                payer_index,
                pre_balances.len(),
                post_balances.len()
            )));
        }

        Ok((pre_balances[payer_index], post_balances[payer_index]))
    }

    fn extract_swap_sol_spent(
        &self,
        tx: &EncodedConfirmedTransactionWithStatusMeta,
        payer: &Pubkey,
    ) -> Result<Option<u64>> {
        if let Some(bonding_curve_sol_spent) =
            self.extract_bonding_curve_sol_spent_from_payer(tx, payer)?
        {
            return Ok(Some(bonding_curve_sol_spent));
        }

        if let Some(largest_inner_swap_transfer) =
            self.extract_largest_inner_swap_transfer_lamports_from_payer(tx, payer)?
        {
            return Ok(Some(largest_inner_swap_transfer));
        }

        self.extract_total_outgoing_transfer_lamports_from_payer(tx, payer)
    }

    fn extract_bonding_curve_sol_spent_from_payer(
        &self,
        tx: &EncodedConfirmedTransactionWithStatusMeta,
        payer: &Pubkey,
    ) -> Result<Option<u64>> {
        let swap_instruction_targets = extract_swap_instruction_targets(tx)?;
        if swap_instruction_targets.is_empty() {
            return Ok(None);
        }

        let meta = match tx.transaction.meta.as_ref() {
            Some(meta) => meta,
            None => return Ok(None),
        };
        let OptionSerializer::Some(inner_instruction_sets) = &meta.inner_instructions else {
            return Ok(None);
        };

        let payer_str = payer.to_string();
        let mut total_lamports = 0u64;
        let mut matched_transfers = 0usize;

        for (instruction_index, bonding_curve_account) in swap_instruction_targets {
            let Some(instruction_set) = inner_instruction_sets
                .iter()
                .find(|set| usize::from(set.index) == instruction_index)
            else {
                continue;
            };

            for instruction in &instruction_set.instructions {
                let Some((destination, lamports)) =
                    parsed_system_transfer_from_payer(instruction, &payer_str)
                else {
                    continue;
                };
                if destination != bonding_curve_account.to_string() {
                    continue;
                }

                total_lamports = total_lamports.checked_add(lamports).ok_or_else(|| {
                    TriggerError::Other(format!(
                        "Bonding-curve SOL spend overflow while summing parsed transfers for payer {}",
                        payer
                    ))
                })?;
                matched_transfers = matched_transfers.saturating_add(1);
            }
        }

        if matched_transfers == 0 {
            return Ok(None);
        }

        debug!(
            payer = %payer,
            matched_transfers,
            bonding_curve_sol_spent_lamports = total_lamports,
            "Resolved parsed bonding-curve system transfers from payer"
        );

        Ok(Some(total_lamports))
    }

    fn extract_largest_inner_swap_transfer_lamports_from_payer(
        &self,
        tx: &EncodedConfirmedTransactionWithStatusMeta,
        payer: &Pubkey,
    ) -> Result<Option<u64>> {
        let swap_instruction_indices = extract_swap_instruction_indices(tx)?;
        if swap_instruction_indices.is_empty() {
            return Ok(None);
        }

        let meta = match tx.transaction.meta.as_ref() {
            Some(meta) => meta,
            None => return Ok(None),
        };
        let OptionSerializer::Some(inner_instruction_sets) = &meta.inner_instructions else {
            return Ok(None);
        };

        let payer_str = payer.to_string();
        let mut total_lamports = 0u64;
        let mut matched_instruction_sets = 0usize;

        for instruction_index in swap_instruction_indices {
            let Some(instruction_set) = inner_instruction_sets
                .iter()
                .find(|set| usize::from(set.index) == instruction_index)
            else {
                continue;
            };

            let Some(largest_transfer) = instruction_set
                .instructions
                .iter()
                .filter_map(|instruction| {
                    parsed_system_transfer_from_payer(instruction, &payer_str)
                        .map(|(_, lamports)| lamports)
                })
                .max()
            else {
                continue;
            };

            total_lamports = total_lamports
                .checked_add(largest_transfer)
                .ok_or_else(|| {
                    TriggerError::Other(format!(
                        "Swap SOL spend overflow while summing parsed inner transfers for payer {}",
                        payer
                    ))
                })?;
            matched_instruction_sets = matched_instruction_sets.saturating_add(1);
        }

        if matched_instruction_sets == 0 {
            return Ok(None);
        }

        debug!(
            payer = %payer,
            matched_instruction_sets,
            inner_swap_sol_spent_lamports = total_lamports,
            "Resolved largest parsed inner swap transfers from payer"
        );

        Ok(Some(total_lamports))
    }

    fn extract_total_outgoing_transfer_lamports_from_payer(
        &self,
        tx: &EncodedConfirmedTransactionWithStatusMeta,
        payer: &Pubkey,
    ) -> Result<Option<u64>> {
        let meta = match tx.transaction.meta.as_ref() {
            Some(meta) => meta,
            None => return Ok(None),
        };
        let payer_str = payer.to_string();
        let mut total_lamports = 0u64;
        let mut transfer_count = 0usize;

        if let EncodedTransaction::Json(ui_tx) = &tx.transaction.transaction {
            if let UiMessage::Parsed(parsed_message) = &ui_tx.message {
                for instruction in &parsed_message.instructions {
                    if let Some(lamports) =
                        parsed_system_transfer_lamports_from_payer(instruction, &payer_str)
                    {
                        total_lamports =
                            total_lamports.checked_add(lamports).ok_or_else(|| {
                                TriggerError::Other(format!(
                                    "Swap SOL spend overflow while summing parsed transfers for payer {}",
                                    payer
                                ))
                            })?;
                        transfer_count = transfer_count.saturating_add(1);
                    }
                }
            }
        }

        if let OptionSerializer::Some(inner_instruction_sets) = &meta.inner_instructions {
            for instruction_set in inner_instruction_sets {
                for instruction in &instruction_set.instructions {
                    if let Some(lamports) =
                        parsed_system_transfer_lamports_from_payer(instruction, &payer_str)
                    {
                        total_lamports =
                            total_lamports.checked_add(lamports).ok_or_else(|| {
                                TriggerError::Other(format!(
                                    "Swap SOL spend overflow while summing parsed inner transfers for payer {}",
                                    payer
                                ))
                            })?;
                        transfer_count = transfer_count.saturating_add(1);
                    }
                }
            }
        }

        if transfer_count == 0 {
            return Ok(None);
        }

        debug!(
            payer = %payer,
            transfer_count,
            payer_outgoing_transfer_lamports = total_lamports,
            "Resolved parsed outgoing payer-side system transfers"
        );

        Ok(Some(total_lamports))
    }

    fn extract_pump_fee_recipient(
        &self,
        tx: &EncodedConfirmedTransactionWithStatusMeta,
    ) -> Option<Pubkey> {
        let EncodedTransaction::Json(ui_tx) = &tx.transaction.transaction else {
            return None;
        };
        let UiMessage::Parsed(parsed_message) = &ui_tx.message else {
            return None;
        };

        parsed_message
            .instructions
            .iter()
            .find_map(parsed_pump_fee_recipient)
    }

    /// Resolve the primary token account that actually received the BUY delta.
    ///
    /// This groups balances by `(mint, account_index)` instead of taking the first owner+mint
    /// match. That avoids latching onto stale dust accounts when the signer owns multiple
    /// token accounts for the same mint.
    fn resolve_primary_buy_token_position(
        &self,
        pre_token_balances: &[UiTransactionTokenBalance],
        post_token_balances: &[UiTransactionTokenBalance],
        account_keys: &[Pubkey],
        payer: &Pubkey,
        mint: &Pubkey,
    ) -> Result<ResolvedBuyTokenPosition> {
        let payer_str = payer.to_string();
        let mint_str = mint.to_string();
        let mut by_account: HashMap<u32, (u64, u64, u8, Option<Pubkey>)> = HashMap::new();

        for balance in pre_token_balances.iter().filter(|balance| {
            balance.mint == mint_str
                && matches!(&balance.owner, OptionSerializer::Some(owner) if owner == &payer_str)
        }) {
            let entry = by_account
                .entry(u32::from(balance.account_index))
                .or_insert((
                    0,
                    0,
                    balance.ui_token_amount.decimals,
                    token_balance_program_id(balance),
                ));
            entry.0 = balance
                .ui_token_amount
                .amount
                .parse::<u64>()
                .unwrap_or_default();
            entry.2 = balance.ui_token_amount.decimals;
            entry.3 = entry.3.or(token_balance_program_id(balance));
        }

        for balance in post_token_balances.iter().filter(|balance| {
            balance.mint == mint_str
                && matches!(&balance.owner, OptionSerializer::Some(owner) if owner == &payer_str)
        }) {
            let entry = by_account
                .entry(u32::from(balance.account_index))
                .or_insert((
                    0,
                    0,
                    balance.ui_token_amount.decimals,
                    token_balance_program_id(balance),
                ));
            entry.1 = balance
                .ui_token_amount
                .amount
                .parse::<u64>()
                .unwrap_or_default();
            entry.2 = balance.ui_token_amount.decimals;
            entry.3 = entry.3.or(token_balance_program_id(balance));
        }

        let mut positive_deltas: Vec<ResolvedBuyTokenPosition> = by_account
            .into_iter()
            .filter_map(
                |(account_index, (pre_balance, post_balance, token_decimals, token_program))| {
                    let tokens_received = post_balance.saturating_sub(pre_balance);
                    if tokens_received == 0 {
                        return None;
                    }
                    let token_account = account_keys.get(account_index as usize).copied()?;
                    Some(ResolvedBuyTokenPosition {
                        token_account,
                        token_balance_after_buy: post_balance,
                        tokens_received,
                        token_decimals,
                        token_program,
                    })
                },
            )
            .collect();

        if positive_deltas.is_empty() {
            return Err(TriggerError::Other(format!(
                "No positive token delta found for payer {} and mint {}",
                payer, mint
            )));
        }

        positive_deltas.sort_by(|left, right| {
            right
                .tokens_received
                .cmp(&left.tokens_received)
                .then_with(|| {
                    right
                        .token_balance_after_buy
                        .cmp(&left.token_balance_after_buy)
                })
                .then_with(|| {
                    left.token_account
                        .to_string()
                        .cmp(&right.token_account.to_string())
                })
        });

        if positive_deltas.len() > 1 {
            warn!(
                payer = %payer,
                mint = %mint,
                candidate_count = positive_deltas.len(),
                top_token_account = %positive_deltas[0].token_account,
                top_tokens_received = positive_deltas[0].tokens_received,
                "Multiple positive token deltas detected for BUY; using the primary account with the largest delta"
            );
        }

        let primary = positive_deltas
            .into_iter()
            .next()
            .expect("positive_deltas checked non-empty");

        debug!(
            mint = %mint,
            token_account = %primary.token_account,
            token_balance_after_buy = primary.token_balance_after_buy,
            tokens_received = primary.tokens_received,
            token_decimals = primary.token_decimals,
            token_program = ?primary.token_program,
            "Resolved primary BUY token position"
        );

        Ok(primary)
    }

    fn resolve_primary_sell_token_position(
        &self,
        pre_token_balances: &[UiTransactionTokenBalance],
        post_token_balances: &[UiTransactionTokenBalance],
        account_keys: &[Pubkey],
        payer: &Pubkey,
        mint: &Pubkey,
    ) -> Result<ResolvedSellTokenPosition> {
        let payer_str = payer.to_string();
        let mint_str = mint.to_string();
        let mut by_account: HashMap<u32, (u64, u64, u8, Option<Pubkey>)> = HashMap::new();

        for balance in pre_token_balances.iter().filter(|balance| {
            balance.mint == mint_str
                && matches!(&balance.owner, OptionSerializer::Some(owner) if owner == &payer_str)
        }) {
            let entry = by_account
                .entry(u32::from(balance.account_index))
                .or_insert((
                    0,
                    0,
                    balance.ui_token_amount.decimals,
                    token_balance_program_id(balance),
                ));
            entry.0 = balance
                .ui_token_amount
                .amount
                .parse::<u64>()
                .unwrap_or_default();
            entry.2 = balance.ui_token_amount.decimals;
            entry.3 = entry.3.or(token_balance_program_id(balance));
        }

        for balance in post_token_balances.iter().filter(|balance| {
            balance.mint == mint_str
                && matches!(&balance.owner, OptionSerializer::Some(owner) if owner == &payer_str)
        }) {
            let entry = by_account
                .entry(u32::from(balance.account_index))
                .or_insert((
                    0,
                    0,
                    balance.ui_token_amount.decimals,
                    token_balance_program_id(balance),
                ));
            entry.1 = balance
                .ui_token_amount
                .amount
                .parse::<u64>()
                .unwrap_or_default();
            entry.2 = balance.ui_token_amount.decimals;
            entry.3 = entry.3.or(token_balance_program_id(balance));
        }

        let mut negative_deltas: Vec<ResolvedSellTokenPosition> = by_account
            .into_iter()
            .filter_map(
                |(account_index, (pre_balance, post_balance, token_decimals, token_program))| {
                    let tokens_sold = pre_balance.saturating_sub(post_balance);
                    if tokens_sold == 0 {
                        return None;
                    }
                    let token_account = account_keys.get(account_index as usize).copied()?;
                    Some(ResolvedSellTokenPosition {
                        token_account,
                        token_balance_before_sell: pre_balance,
                        token_balance_after_sell: post_balance,
                        tokens_sold,
                        token_decimals,
                        token_program,
                    })
                },
            )
            .collect();

        if negative_deltas.is_empty() {
            return Err(TriggerError::Other(format!(
                "No negative token delta found for payer {} and mint {}",
                payer, mint
            )));
        }

        negative_deltas.sort_by(|left, right| {
            right
                .tokens_sold
                .cmp(&left.tokens_sold)
                .then_with(|| {
                    right
                        .token_balance_before_sell
                        .cmp(&left.token_balance_before_sell)
                })
                .then_with(|| {
                    left.token_account
                        .to_string()
                        .cmp(&right.token_account.to_string())
                })
        });

        if negative_deltas.len() > 1 {
            warn!(
                payer = %payer,
                mint = %mint,
                candidate_count = negative_deltas.len(),
                top_token_account = %negative_deltas[0].token_account,
                top_tokens_sold = negative_deltas[0].tokens_sold,
                "Multiple negative token deltas detected for SELL; using the primary account with the largest delta"
            );
        }

        let primary = negative_deltas
            .into_iter()
            .next()
            .expect("negative_deltas checked non-empty");

        debug!(
            mint = %mint,
            token_account = %primary.token_account,
            token_balance_before_sell = primary.token_balance_before_sell,
            token_balance_after_sell = primary.token_balance_after_sell,
            tokens_sold = primary.tokens_sold,
            token_decimals = primary.token_decimals,
            token_program = ?primary.token_program,
            "Resolved primary SELL token position"
        );

        Ok(primary)
    }
}

fn extract_transaction_account_keys(
    tx: &EncodedConfirmedTransactionWithStatusMeta,
) -> Result<Vec<Pubkey>> {
    extract_encoded_transaction_account_keys(&tx.transaction.transaction)
}

fn extract_encoded_transaction_account_keys(
    encoded_tx: &EncodedTransaction,
) -> Result<Vec<Pubkey>> {
    let account_keys = match encoded_tx {
        EncodedTransaction::Json(ui_tx) => match &ui_tx.message {
            UiMessage::Raw(raw_msg) => raw_msg
                .account_keys
                .iter()
                .map(|key| {
                    Pubkey::from_str(key).map_err(|error| {
                        TriggerError::Other(format!(
                            "Failed to parse raw confirmed transaction account key {}: {}",
                            key, error
                        ))
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            UiMessage::Parsed(parsed_msg) => parsed_msg
                .account_keys
                .iter()
                .map(|account| {
                    Pubkey::from_str(&account.pubkey).map_err(|error| {
                        TriggerError::Other(format!(
                            "Failed to parse parsed confirmed transaction account key {}: {}",
                            account.pubkey, error
                        ))
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        },
        EncodedTransaction::Accounts(account_list) => account_list
            .account_keys
            .iter()
            .map(|account| {
                Pubkey::from_str(&account.pubkey).map_err(|error| {
                    TriggerError::Other(format!(
                        "Failed to parse account-list confirmed transaction account key {}: {}",
                        account.pubkey, error
                    ))
                })
            })
            .collect::<Result<Vec<_>>>()?,
        EncodedTransaction::LegacyBinary(_) | EncodedTransaction::Binary(_, _) => {
            let decoded = encoded_tx.decode().ok_or_else(|| {
                TriggerError::Other("Failed to decode confirmed transaction".to_string())
            })?;
            decoded.message.static_account_keys().to_vec()
        }
    };

    if account_keys.is_empty() {
        return Err(TriggerError::Other(
            "Confirmed transaction contains no account keys".to_string(),
        ));
    }

    Ok(account_keys)
}

fn token_balance_program_id(balance: &UiTransactionTokenBalance) -> Option<Pubkey> {
    let value = serde_json::to_value(balance).ok()?;
    parse_optional_pubkey_field(&value, &["programId", "program_id"])
}

fn extract_swap_instruction_targets(
    tx: &EncodedConfirmedTransactionWithStatusMeta,
) -> Result<Vec<(usize, Pubkey)>> {
    let EncodedTransaction::Json(ui_tx) = &tx.transaction.transaction else {
        return Ok(Vec::new());
    };
    let UiMessage::Parsed(parsed_message) = &ui_tx.message else {
        return Ok(Vec::new());
    };

    Ok(parsed_message
        .instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| {
            parsed_swap_instruction_bonding_curve_account(instruction)
                .map(|account| (index, account))
        })
        .collect())
}

fn extract_swap_instruction_indices(
    tx: &EncodedConfirmedTransactionWithStatusMeta,
) -> Result<Vec<usize>> {
    let EncodedTransaction::Json(ui_tx) = &tx.transaction.transaction else {
        return Ok(Vec::new());
    };
    let UiMessage::Parsed(parsed_message) = &ui_tx.message else {
        return Ok(Vec::new());
    };

    Ok(parsed_message
        .instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| is_swap_instruction(instruction).then_some(index))
        .collect())
}

fn parsed_swap_instruction_bonding_curve_account(instruction: &UiInstruction) -> Option<Pubkey> {
    let UiInstruction::Parsed(parsed_instruction) = instruction else {
        return None;
    };
    let UiParsedInstruction::PartiallyDecoded(decoded) = parsed_instruction else {
        return None;
    };
    if !is_supported_swap_program(&decoded.program_id) {
        return None;
    }

    Pubkey::from_str(decoded.accounts.get(3)?).ok()
}

fn is_swap_instruction(instruction: &UiInstruction) -> bool {
    let UiInstruction::Parsed(parsed_instruction) = instruction else {
        return false;
    };
    let UiParsedInstruction::PartiallyDecoded(decoded) = parsed_instruction else {
        return false;
    };

    is_supported_swap_program(&decoded.program_id)
}

fn is_supported_swap_program(program_id: &str) -> bool {
    matches!(
        program_id,
        crate::validation::PUMP_PROGRAM_ID | crate::validation::BONK_PROGRAM_ID
    )
}

fn parsed_system_transfer_from_payer(
    instruction: &UiInstruction,
    payer: &str,
) -> Option<(String, u64)> {
    let UiInstruction::Parsed(parsed_instruction) = instruction else {
        return None;
    };
    let UiParsedInstruction::Parsed(fully_parsed) = parsed_instruction else {
        return None;
    };
    if fully_parsed.program_id != system_program::id().to_string() {
        return None;
    }

    let parsed = &fully_parsed.parsed;
    if parsed.get("type")?.as_str()? != "transfer" {
        return None;
    }

    let info = parsed.get("info")?;
    if info.get("source")?.as_str()? != payer {
        return None;
    }

    Some((
        info.get("destination")?.as_str()?.to_string(),
        parse_u64_value(info.get("lamports")?)?,
    ))
}

fn parsed_system_transfer_lamports_from_payer(
    instruction: &UiInstruction,
    payer: &str,
) -> Option<u64> {
    parsed_system_transfer_from_payer(instruction, payer).map(|(_, lamports)| lamports)
}

fn parsed_pump_fee_recipient(instruction: &UiInstruction) -> Option<Pubkey> {
    let UiInstruction::Parsed(parsed_instruction) = instruction else {
        return None;
    };
    let UiParsedInstruction::PartiallyDecoded(decoded) = parsed_instruction else {
        return None;
    };
    if decoded.program_id != crate::validation::PUMP_PROGRAM_ID {
        return None;
    }

    Pubkey::from_str(decoded.accounts.get(1)?).ok()
}

fn parse_optional_pubkey_field(value: &Value, keys: &[&str]) -> Option<Pubkey> {
    keys.iter()
        .filter_map(|key| value.get(*key))
        .find_map(|candidate| candidate.as_str())
        .and_then(|pubkey| Pubkey::from_str(pubkey).ok())
}

fn parse_u64_value(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|raw| raw.parse::<u64>().ok()))
}

/// Simplified entry price extraction for direct use
///
/// This function can be called directly after a buy transaction is confirmed.
pub async fn extract_entry_price_after_buy(
    rpc_client: Arc<RpcClient>,
    signature: &Signature,
    payer: &Pubkey,
    mint: &Pubkey,
) -> Result<BuyTransactionMetadata> {
    let monitor = TransactionMonitor::new(rpc_client);
    let tx = monitor.fetch_transaction(signature).await?;
    monitor.extract_buy_metadata(&tx, payer, mint, *signature)
}

/// Simplified realized exit price extraction for direct use after SELL confirmation.
pub async fn extract_exit_price_after_sell(
    rpc_client: Arc<RpcClient>,
    signature: &Signature,
    payer: &Pubkey,
    mint: &Pubkey,
) -> Result<SellTransactionMetadata> {
    let monitor = TransactionMonitor::new(rpc_client);
    let tx = monitor.fetch_transaction(signature).await?;
    monitor.extract_sell_metadata(&tx, payer, mint, *signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use solana_transaction_status::{
        parse_accounts::ParsedAccount, EncodedConfirmedTransactionWithStatusMeta,
        EncodedTransaction, EncodedTransactionWithStatusMeta, UiInstruction, UiMessage,
        UiParsedInstruction, UiParsedMessage, UiPartiallyDecodedInstruction, UiTransaction,
        UiTransactionStatusMeta,
    };

    #[test]
    fn test_calculate_entry_price() {
        // Test case: 1 SOL spent (1e9 lamports) for 1 million tokens
        let sol_spent = 1_000_000_000; // 1 SOL
        let tokens_received = 1_000_000; // 1M tokens

        let result = BuyTransactionMetadata::calculate_entry_price(sol_spent, tokens_received);
        assert!(result.is_ok());

        // Expected: 1 SOL / 1M tokens = 1000 lamports per token
        // With 1e9 scaling: 1000 * 1e9 = 1_000_000_000_000
        let entry_price = result.unwrap();
        assert_eq!(entry_price, 1_000_000_000_000);
    }

    #[test]
    fn test_calculate_entry_price_small_amount() {
        // Test case: 0.1 SOL spent for 10 million tokens
        let sol_spent = 100_000_000; // 0.1 SOL
        let tokens_received = 10_000_000; // 10M tokens

        let result = BuyTransactionMetadata::calculate_entry_price(sol_spent, tokens_received);
        assert!(result.is_ok());

        // Expected: 0.1 SOL / 10M tokens = 0.00000001 SOL per token
        // With 1e9 scaling: 0.00000001 * 1e9 = 10
        // Actually: (100_000_000 * 1e9) / 10_000_000 = 10_000_000_000
        let entry_price = result.unwrap();
        assert_eq!(entry_price, 10_000_000_000);
    }

    #[test]
    fn test_calculate_entry_price_zero_tokens() {
        let sol_spent = 1_000_000_000;
        let tokens_received = 0;

        let result = BuyTransactionMetadata::calculate_entry_price(sol_spent, tokens_received);
        assert!(result.is_err());
    }

    #[test]
    fn test_buy_transaction_metadata() {
        let metadata = BuyTransactionMetadata {
            signature: Signature::default(),
            mint: Pubkey::new_unique(),
            sol_spent: 500_000_000,
            tokens_received: 25_000_000,
            entry_price: 20_000_000_000,
            slot: 12345,
            token_account: Pubkey::new_unique(),
            token_balance_after_buy: 25_000_000,
            token_decimals: 6,
            token_program: Some(
                Pubkey::from_str("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb")
                    .expect("token-2022"),
            ),
            fee_recipient: Some(Pubkey::new_unique()),
        };

        assert_eq!(metadata.slot, 12345);
        assert_eq!(metadata.sol_spent, 500_000_000);
        assert_eq!(metadata.tokens_received, 25_000_000);
        assert!(metadata.fee_recipient.is_some());
    }

    #[test]
    fn test_sol_spent_calculation() {
        let monitor = TransactionMonitor::new(Arc::new(RpcClient::new(
            "http://localhost:8899".to_string(),
        )));

        let pre_balances = vec![5_000_000_000, 1_000_000_000]; // 5 SOL, 1 SOL
        let post_balances = vec![4_500_000_000, 1_000_000_000]; // 4.5 SOL, 1 SOL

        let result = monitor.calculate_sol_spent(&pre_balances, &post_balances, 0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 500_000_000); // 0.5 SOL spent
    }

    #[test]
    fn test_sol_spent_out_of_bounds() {
        let monitor = TransactionMonitor::new(Arc::new(RpcClient::new(
            "http://localhost:8899".to_string(),
        )));

        let pre_balances = vec![5_000_000_000];
        let post_balances = vec![4_500_000_000];

        let result = monitor.calculate_sol_spent(&pre_balances, &post_balances, 5);
        assert!(result.is_err());
    }

    fn test_status_meta(
        pre_balances: Vec<u64>,
        post_balances: Vec<u64>,
        pre_token_balances: Vec<UiTransactionTokenBalance>,
        post_token_balances: Vec<UiTransactionTokenBalance>,
        inner_instructions: serde_json::Value,
    ) -> UiTransactionStatusMeta {
        serde_json::from_value(json!({
            "err": null,
            "status": { "Ok": null },
            "fee": 5_000,
            "preBalances": pre_balances,
            "postBalances": post_balances,
            "innerInstructions": inner_instructions,
            "logMessages": [],
            "preTokenBalances": pre_token_balances,
            "postTokenBalances": post_token_balances,
            "rewards": [],
            "loadedAddresses": {
                "writable": [],
                "readonly": []
            },
            "computeUnitsConsumed": 12345
        }))
        .expect("valid transaction meta")
    }

    #[test]
    fn test_extract_buy_metadata_uses_bonding_curve_sol_spent_and_ignores_tip() {
        let monitor = TransactionMonitor::new(Arc::new(RpcClient::new(
            "http://localhost:8899".to_string(),
        )));
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_account = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let signature = Signature::new_unique();
        let protocol_fee_account = Pubkey::new_unique();
        let creator_vault = Pubkey::new_unique();
        let tip_account = Pubkey::new_unique();
        let program_id = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

        let tx = EncodedConfirmedTransactionWithStatusMeta {
            slot: 411_191_637,
            transaction: EncodedTransactionWithStatusMeta {
                transaction: EncodedTransaction::Json(UiTransaction {
                    signatures: vec![signature.to_string()],
                    message: UiMessage::Parsed(UiParsedMessage {
                        account_keys: vec![
                            ParsedAccount {
                                pubkey: payer.to_string(),
                                writable: true,
                                signer: true,
                                source: None,
                            },
                            ParsedAccount {
                                pubkey: token_account.to_string(),
                                writable: true,
                                signer: false,
                                source: None,
                            },
                        ],
                        recent_blockhash: "11111111111111111111111111111111".to_string(),
                        instructions: vec![
                            UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(
                                UiPartiallyDecodedInstruction {
                                    program_id: crate::validation::PUMP_PROGRAM_ID.to_string(),
                                    accounts: vec![
                                        Pubkey::new_unique().to_string(),
                                        protocol_fee_account.to_string(),
                                        mint.to_string(),
                                        bonding_curve.to_string(),
                                    ],
                                    data: bs58::encode([0u8; 8]).into_string(),
                                    stack_height: None,
                                },
                            )),
                            test_system_transfer_instruction(payer, tip_account, 2_586_132),
                        ],
                        address_table_lookups: None,
                    }),
                }),
                meta: Some(test_status_meta(
                    vec![10_000_000, 0],
                    vec![5_386_383, 0],
                    vec![test_ui_token_balance(1, mint, payer, 0, 6, program_id)],
                    vec![test_ui_token_balance(
                        1,
                        mint,
                        payer,
                        2_949_483_952,
                        6,
                        program_id,
                    )],
                    json!([
                        {
                            "index": 0,
                            "instructions": [
                                {
                                    "parsed": {
                                        "info": {
                                            "source": payer.to_string(),
                                            "destination": creator_vault.to_string(),
                                            "lamports": 172
                                        },
                                        "type": "transfer"
                                    },
                                    "program": "system",
                                    "programId": system_program::id().to_string(),
                                    "stackHeight": 3
                                },
                                {
                                    "parsed": {
                                        "info": {
                                            "source": payer.to_string(),
                                            "destination": bonding_curve.to_string(),
                                            "lamports": 57_118
                                        },
                                        "type": "transfer"
                                    },
                                    "program": "system",
                                    "programId": system_program::id().to_string(),
                                    "stackHeight": 3
                                },
                                {
                                    "parsed": {
                                        "info": {
                                            "source": payer.to_string(),
                                            "destination": protocol_fee_account.to_string(),
                                            "lamports": 543
                                        },
                                        "type": "transfer"
                                    },
                                    "program": "system",
                                    "programId": system_program::id().to_string(),
                                    "stackHeight": 3
                                }
                            ]
                        }
                    ]),
                )),
                version: None,
            },
            block_time: None,
        };

        let metadata = monitor
            .extract_buy_metadata(&tx, &payer, &mint, signature)
            .expect("buy metadata should extract");

        assert_eq!(
            metadata.sol_spent, 57_118,
            "entry price must use only the bonding-curve transfer instead of inline tip or sidecar fees"
        );
        assert_eq!(metadata.tokens_received, 2_949_483_952);
        assert_eq!(metadata.entry_price, 19_365);
    }

    fn test_ui_token_balance(
        account_index: u8,
        mint: Pubkey,
        owner: Pubkey,
        amount: u64,
        decimals: u8,
        program_id: &str,
    ) -> UiTransactionTokenBalance {
        let divisor = 10f64.powi(i32::from(decimals));
        let ui_amount = amount as f64 / divisor;
        serde_json::from_value(json!({
            "accountIndex": account_index,
            "mint": mint.to_string(),
            "uiTokenAmount": {
                "uiAmount": ui_amount,
                "decimals": decimals,
                "amount": amount.to_string(),
                "uiAmountString": format!("{ui_amount:.6}")
            },
            "owner": owner.to_string(),
            "programId": program_id
        }))
        .expect("valid token balance")
    }

    #[test]
    fn test_resolve_primary_buy_token_position_prefers_largest_positive_delta_account() {
        let monitor = TransactionMonitor::new(Arc::new(RpcClient::new(
            "http://localhost:8899".to_string(),
        )));
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let dust_account = Pubkey::new_unique();
        let primary_account = Pubkey::new_unique();
        let program_id = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

        let pre = vec![
            test_ui_token_balance(1, mint, payer, 0, 6, program_id),
            test_ui_token_balance(2, mint, payer, 0, 6, program_id),
        ];
        let post = vec![
            test_ui_token_balance(1, mint, payer, 1, 6, program_id),
            test_ui_token_balance(2, mint, payer, 1_250_000, 6, program_id),
        ];
        let account_keys = vec![payer, dust_account, primary_account];

        let resolved = monitor
            .resolve_primary_buy_token_position(&pre, &post, &account_keys, &payer, &mint)
            .expect("primary buy token position");

        assert_eq!(resolved.token_account, primary_account);
        assert_eq!(resolved.token_balance_after_buy, 1_250_000);
        assert_eq!(resolved.tokens_received, 1_250_000);
        assert_eq!(resolved.token_decimals, 6);
        assert_eq!(
            resolved.token_program,
            Some(Pubkey::from_str(program_id).expect("token program"))
        );
    }

    #[test]
    fn test_extract_transaction_account_keys_supports_json_parsed_transactions() {
        let payer = Pubkey::new_unique();
        let token_account = Pubkey::new_unique();
        let tx = EncodedConfirmedTransactionWithStatusMeta {
            slot: 42,
            transaction: EncodedTransactionWithStatusMeta {
                transaction: EncodedTransaction::Json(UiTransaction {
                    signatures: vec!["sig".to_string()],
                    message: UiMessage::Parsed(UiParsedMessage {
                        account_keys: vec![
                            ParsedAccount {
                                pubkey: payer.to_string(),
                                writable: true,
                                signer: true,
                                source: None,
                            },
                            ParsedAccount {
                                pubkey: token_account.to_string(),
                                writable: true,
                                signer: false,
                                source: None,
                            },
                        ],
                        recent_blockhash: "11111111111111111111111111111111".to_string(),
                        instructions: vec![],
                        address_table_lookups: None,
                    }),
                }),
                meta: None,
                version: None,
            },
            block_time: None,
        };

        let account_keys = extract_transaction_account_keys(&tx).expect("account keys");

        assert_eq!(account_keys, vec![payer, token_account]);
    }

    #[test]
    fn test_extract_buy_metadata_resolves_pump_fee_recipient() {
        let monitor = TransactionMonitor::new(Arc::new(RpcClient::new(
            "http://localhost:8899".to_string(),
        )));
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_account = Pubkey::new_unique();
        let fee_recipient = Pubkey::new_unique();
        let signature = Signature::new_unique();
        let program_id = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

        let tx = EncodedConfirmedTransactionWithStatusMeta {
            slot: 42,
            transaction: EncodedTransactionWithStatusMeta {
                transaction: EncodedTransaction::Json(UiTransaction {
                    signatures: vec![signature.to_string()],
                    message: UiMessage::Parsed(UiParsedMessage {
                        account_keys: vec![
                            ParsedAccount {
                                pubkey: payer.to_string(),
                                writable: true,
                                signer: true,
                                source: None,
                            },
                            ParsedAccount {
                                pubkey: token_account.to_string(),
                                writable: true,
                                signer: false,
                                source: None,
                            },
                        ],
                        recent_blockhash: "11111111111111111111111111111111".to_string(),
                        instructions: vec![UiInstruction::Parsed(
                            UiParsedInstruction::PartiallyDecoded(UiPartiallyDecodedInstruction {
                                program_id: crate::validation::PUMP_PROGRAM_ID.to_string(),
                                accounts: vec![
                                    Pubkey::new_unique().to_string(),
                                    fee_recipient.to_string(),
                                    mint.to_string(),
                                ],
                                data: bs58::encode([0u8; 8]).into_string(),
                                stack_height: None,
                            }),
                        )],
                        address_table_lookups: None,
                    }),
                }),
                meta: Some(test_status_meta(
                    vec![1_000_000_000, 0],
                    vec![900_000_000, 0],
                    vec![test_ui_token_balance(1, mint, payer, 0, 6, program_id)],
                    vec![test_ui_token_balance(
                        1, mint, payer, 1_250_000, 6, program_id,
                    )],
                    json!([]),
                )),
                version: None,
            },
            block_time: None,
        };

        let metadata = monitor
            .extract_buy_metadata(&tx, &payer, &mint, signature)
            .expect("buy metadata should extract");

        assert_eq!(metadata.fee_recipient, Some(fee_recipient));
    }

    #[test]
    fn test_calculate_exit_price() {
        let result = SellTransactionMetadata::calculate_exit_price(200_000_000, 1_750_000);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 114_285_714_285);
    }

    fn test_system_transfer_instruction(
        source: Pubkey,
        destination: Pubkey,
        lamports: u64,
    ) -> UiInstruction {
        serde_json::from_value(json!({
            "parsed": {
                "type": "transfer",
                "info": {
                    "source": source.to_string(),
                    "destination": destination.to_string(),
                    "lamports": lamports
                }
            },
            "program": "system",
            "programId": system_program::id().to_string(),
            "stackHeight": null
        }))
        .expect("valid parsed system transfer")
    }

    #[test]
    fn test_extract_sell_metadata_uses_gross_swap_proceeds() {
        let monitor = TransactionMonitor::new(Arc::new(RpcClient::new(
            "http://localhost:8899".to_string(),
        )));
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_account = Pubkey::new_unique();
        let tip_account = Pubkey::new_unique();
        let signature = Signature::new_unique();
        let program_id = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

        let tx = EncodedConfirmedTransactionWithStatusMeta {
            slot: 411_952_999,
            transaction: EncodedTransactionWithStatusMeta {
                transaction: EncodedTransaction::Json(UiTransaction {
                    signatures: vec![signature.to_string()],
                    message: UiMessage::Parsed(UiParsedMessage {
                        account_keys: vec![
                            ParsedAccount {
                                pubkey: payer.to_string(),
                                writable: true,
                                signer: true,
                                source: None,
                            },
                            ParsedAccount {
                                pubkey: token_account.to_string(),
                                writable: true,
                                signer: false,
                                source: None,
                            },
                        ],
                        recent_blockhash: "11111111111111111111111111111111".to_string(),
                        instructions: vec![test_system_transfer_instruction(
                            payer,
                            tip_account,
                            1_200_000,
                        )],
                        address_table_lookups: None,
                    }),
                }),
                meta: Some(test_status_meta(
                    vec![1_000_000_000, 0],
                    vec![1_198_795_000, 0],
                    vec![test_ui_token_balance(
                        1, mint, payer, 1_750_000, 6, program_id,
                    )],
                    vec![],
                    json!([]),
                )),
                version: None,
            },
            block_time: None,
        };

        let metadata = monitor
            .extract_sell_metadata(&tx, &payer, &mint, signature)
            .expect("sell metadata should extract");

        assert_eq!(metadata.sol_received, 200_000_000);
        assert_eq!(metadata.payer_wallet_net_change, 198_795_000);
        assert_eq!(metadata.network_fee_lamports, 5_000);
        assert_eq!(metadata.payer_outgoing_transfer_lamports, 1_200_000);
        assert_eq!(metadata.tokens_sold, 1_750_000);
        assert_eq!(metadata.token_account, token_account);
        assert_eq!(metadata.token_balance_before_sell, 1_750_000);
        assert_eq!(metadata.token_balance_after_sell, 0);
        assert_eq!(metadata.exit_price, 114_285_714_285);
    }

    #[test]
    fn test_resolve_primary_sell_token_position_prefers_largest_negative_delta_account() {
        let monitor = TransactionMonitor::new(Arc::new(RpcClient::new(
            "http://localhost:8899".to_string(),
        )));
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let dust_account = Pubkey::new_unique();
        let primary_account = Pubkey::new_unique();
        let program_id = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

        let pre = vec![
            test_ui_token_balance(1, mint, payer, 10, 6, program_id),
            test_ui_token_balance(2, mint, payer, 1_250_000, 6, program_id),
        ];
        let post = vec![
            test_ui_token_balance(1, mint, payer, 9, 6, program_id),
            test_ui_token_balance(2, mint, payer, 0, 6, program_id),
        ];
        let account_keys = vec![payer, dust_account, primary_account];

        let resolved = monitor
            .resolve_primary_sell_token_position(&pre, &post, &account_keys, &payer, &mint)
            .expect("primary sell token position");

        assert_eq!(resolved.token_account, primary_account);
        assert_eq!(resolved.token_balance_before_sell, 1_250_000);
        assert_eq!(resolved.token_balance_after_sell, 0);
        assert_eq!(resolved.tokens_sold, 1_250_000);
        assert_eq!(resolved.token_decimals, 6);
        assert_eq!(
            resolved.token_program,
            Some(Pubkey::from_str(program_id).expect("token program"))
        );
    }
}
