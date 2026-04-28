//! Revolver Integration - Magazine Creation After BUY
//!
//! This module provides helpers for creating SELL bullet magazines after
//! a successful BUY transaction. It generates pre-signed SELL transactions
//! at various price targets based on the position acquired.
//!
//! # Usage
//!
//! ```ignore
//! let magazine = create_magazine_after_buy(
//!     &payer,
//!     mint,
//!     position_size,
//!     entry_price,
//!     &targets,
//!     &rpc_client,
//! ).await?;
//!
//! revolver.load_magazine(mint, magazine);
//! ```

use crate::direct_sell_builder::DirectSellBuilder;
use crate::errors::{Result, TriggerError};
use crate::revolver::Bullet;
use crate::revolver_sell_builder::{AmmProtocol, SellTxBuilder};
use ghost_core::SwapPlanBuilder;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    hash::Hash,
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    transaction::VersionedTransaction,
};
use std::str::FromStr;
use tracing::{debug, info};

/// Default slippage margin for Revolver sell bullets (5%)
const DEFAULT_REVOLVER_SLIPPAGE_BPS: u16 = 500;
/// Default time stop for SELL bullets (20 minutes)
const DEFAULT_TIME_STOP_SECS: u64 = 20 * 60;

/// Price target configuration for a bullet
#[derive(Debug, Clone)]
pub struct PriceTarget {
    /// Target price multiplier (e.g., 1.5 = 150% of entry price)
    pub price_multiplier: f64,
    /// Position fraction to sell at this target (0-10000 bps)
    pub position_fraction_bps: u16,
}

impl PriceTarget {
    /// Create a new price target
    pub fn new(price_multiplier: f64, position_fraction_bps: u16) -> Result<Self> {
        if position_fraction_bps > 10000 {
            return Err(TriggerError::ConfigError(
                "Position fraction must be between 0 and 10000 bps".to_string(),
            ));
        }

        if price_multiplier <= 0.0 {
            return Err(TriggerError::ConfigError(
                "Price multiplier must be positive".to_string(),
            ));
        }

        Ok(Self {
            price_multiplier,
            position_fraction_bps,
        })
    }

    /// Calculate the target price in lamports
    pub fn calculate_target_price(&self, entry_price: u64) -> u64 {
        ((entry_price as f64) * self.price_multiplier) as u64
    }
}

/// Configuration for magazine creation
#[derive(Debug, Clone)]
pub struct MagazineConfig {
    /// Price targets for bullets
    pub targets: Vec<PriceTarget>,
    /// Program ID for the swap
    pub program_id: Pubkey,
    /// Whether to validate total position fractions sum to 100%
    pub validate_total_fraction: bool,
    /// Optional time stop applied to each bullet
    pub time_stop_secs: Option<u64>,
}

impl MagazineConfig {
    /// Create default magazine config with common targets
    /// - 25% at 2x
    /// - 25% at 3x
    /// - 50% at 5x
    pub fn default_targets(program_id: Pubkey) -> Self {
        Self {
            targets: vec![
                PriceTarget::new(2.0, 2500).unwrap(), // 25% at 2x
                PriceTarget::new(3.0, 2500).unwrap(), // 25% at 3x
                PriceTarget::new(5.0, 5000).unwrap(), // 50% at 5x
            ],
            program_id,
            validate_total_fraction: true,
            time_stop_secs: Some(DEFAULT_TIME_STOP_SECS),
        }
    }

    /// Validate that targets sum to 100% (10000 bps)
    pub fn validate(&self) -> Result<()> {
        if !self.validate_total_fraction {
            return Ok(());
        }

        let total_bps: u32 = self
            .targets
            .iter()
            .map(|t| t.position_fraction_bps as u32)
            .sum();

        if total_bps != 10000 {
            return Err(TriggerError::ConfigError(format!(
                "Total position fractions must equal 10000 bps (100%), got {} bps",
                total_bps
            )));
        }

        Ok(())
    }
}

/// Create a magazine of SELL bullets after a successful BUY
pub async fn create_magazine_after_buy(
    payer: &Keypair,
    mint: Pubkey,
    position_size: u64,
    entry_price: u64,
    config: &MagazineConfig,
    rpc_client: &RpcClient,
) -> Result<Vec<Bullet>> {
    if position_size == 0 {
        return Err(TriggerError::ConfigError(
            "Position size must be greater than zero".to_string(),
        ));
    }
    if entry_price == 0 {
        return Err(TriggerError::ConfigError(
            "Entry price must be greater than zero".to_string(),
        ));
    }

    info!(
        "Creating magazine for mint {} with position size {} at entry price {}",
        mint, position_size, entry_price
    );

    // Validate configuration
    config.validate()?;

    // Get fresh blockhash
    let blockhash = rpc_client
        .get_latest_blockhash()
        .await
        .map_err(|e| TriggerError::ClientError(e))?;

    let mut bullets = Vec::new();

    for target in &config.targets {
        let target_price = target.calculate_target_price(entry_price);
        let bullet_amount =
            (position_size as f64 * (target.position_fraction_bps as f64 / 10000.0)) as u64;
        if bullet_amount == 0 {
            debug!(
                "Skipping bullet with zero amount: target_price={}, fraction_bps={}",
                target_price, target.position_fraction_bps
            );
            continue;
        }
        let min_output = SellTxBuilder::calculate_min_output(
            bullet_amount,
            target_price,
            DEFAULT_REVOLVER_SLIPPAGE_BPS,
        )?;

        debug!(
            "Creating bullet: target_price={}, amount={}, min_output={}, fraction_bps={}",
            target_price, bullet_amount, min_output, target.position_fraction_bps
        );

        // Create SELL swap plan
        let swap_plan = SwapPlanBuilder::new(payer.pubkey(), config.program_id)
            .amount_in(bullet_amount)
            .min_amount_out(min_output)
            .timeout_seconds(300)
            .with_strategy("revolver_sell")
            .build()
            .map_err(|e| TriggerError::Other(format!("Failed to build swap plan: {}", e)))?;

        // Build and sign transaction
        let tx_bytes =
            build_sell_transaction(&swap_plan, mint, payer, blockhash, config.program_id).await?;

        // Create bullet
        let bullet = Bullet::new(tx_bytes, target_price, target.position_fraction_bps)?
            .with_time_stop(config.time_stop_secs);

        bullets.push(bullet);
    }

    info!("Created {} bullets for mint {}", bullets.len(), mint);
    Ok(bullets)
}

/// Create a virtual magazine with the same target/fraction model but no signed tx bytes.
///
/// Used by mirrored shadow mode so post-buy management can reuse the exact same
/// `TokenRevolver` semantics without producing on-chain side effects.
pub fn create_virtual_magazine(entry_price: u64, config: &MagazineConfig) -> Result<Vec<Bullet>> {
    if entry_price == 0 {
        return Err(TriggerError::ConfigError(
            "Entry price must be greater than zero".to_string(),
        ));
    }

    config.validate()?;

    let mut bullets = Vec::new();
    for target in &config.targets {
        let target_price = target.calculate_target_price(entry_price);
        bullets.push(
            Bullet::new(Vec::new(), target_price, target.position_fraction_bps)?
                .with_time_stop(config.time_stop_secs),
        );
    }
    bullets.sort_by_key(|bullet| bullet.target_price);
    Ok(bullets)
}

/// Build a SELL transaction
async fn build_sell_transaction(
    swap_plan: &ghost_core::SwapPlan,
    mint: Pubkey,
    payer: &Keypair,
    blockhash: Hash,
    program_id: Pubkey,
) -> Result<Vec<u8>> {
    // Build a direct SELL instruction using the configured AMM program
    let bonk_program = Pubkey::from_str(crate::validation::BONK_PROGRAM_ID)
        .map_err(|e| TriggerError::InvalidPool(format!("Invalid Bonk program id: {}", e)))?;

    if program_id == bonk_program {
        let payer_owned = Keypair::from_bytes(&payer.to_bytes()).map_err(|e| {
            TriggerError::SerializationError(format!("Failed to clone payer: {}", e))
        })?;
        let builder = SellTxBuilder::with_default_config(payer_owned);
        return builder
            .build_signed_sell_tx(
                mint,
                None,
                swap_plan.amount_in,
                swap_plan.min_amount_out,
                blockhash,
                AmmProtocol::BonkFun,
            )
            .await;
    }

    let sell_instruction = if program_id == DirectSellBuilder::pump_program_id() {
        DirectSellBuilder::build_sell_ix(
            &payer.pubkey(),
            &mint,
            swap_plan.amount_in,      // Amount of tokens to sell
            swap_plan.min_amount_out, // Minimum SOL to receive
        )
    } else {
        return Err(TriggerError::InvalidPool(format!(
            "Unsupported AMM program for sell: {}",
            program_id
        )));
    };

    // Build versioned transaction with the sell instruction
    let message = v0::Message::try_compile(
        &payer.pubkey(),
        &[sell_instruction],
        &[], // No LUT for now - can be added later for size optimization
        blockhash,
    )
    .map_err(|e| TriggerError::TransactionBuildFailed(e.to_string()))?;

    let versioned_message = VersionedMessage::V0(message);

    // Sign the transaction
    let tx = VersionedTransaction::try_new(versioned_message, &[payer])
        .map_err(|e| TriggerError::TransactionBuildFailed(e.to_string()))?;

    // Serialize
    bincode::serialize(&tx).map_err(|e| TriggerError::SerializationError(e.to_string()))
}

/// Helper to create a simple magazine with standard targets
pub async fn create_standard_magazine(
    payer: &Keypair,
    mint: Pubkey,
    position_size: u64,
    entry_price: u64,
    program_id: Pubkey,
    rpc_client: &RpcClient,
) -> Result<Vec<Bullet>> {
    let config = MagazineConfig::default_targets(program_id);
    create_magazine_after_buy(payer, mint, position_size, entry_price, &config, rpc_client).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_target_creation() {
        let target = PriceTarget::new(2.0, 5000).unwrap();
        assert_eq!(target.price_multiplier, 2.0);
        assert_eq!(target.position_fraction_bps, 5000);
    }

    #[test]
    fn test_price_target_invalid_fraction() {
        let result = PriceTarget::new(2.0, 10001);
        assert!(result.is_err());
    }

    #[test]
    fn test_price_target_calculate() {
        let target = PriceTarget::new(2.5, 5000).unwrap();
        let entry_price = 1000;
        assert_eq!(target.calculate_target_price(entry_price), 2500);
    }

    #[test]
    fn test_magazine_config_default() {
        let program_id = Pubkey::new_unique();
        let config = MagazineConfig::default_targets(program_id);
        assert_eq!(config.targets.len(), 3);
        assert!(config.validate_total_fraction);
    }

    #[test]
    fn test_magazine_config_validate_success() {
        let program_id = Pubkey::new_unique();
        let config = MagazineConfig {
            targets: vec![
                PriceTarget::new(2.0, 3000).unwrap(),
                PriceTarget::new(3.0, 7000).unwrap(),
            ],
            program_id,
            validate_total_fraction: true,
            time_stop_secs: Some(DEFAULT_TIME_STOP_SECS),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_magazine_config_validate_failure() {
        let program_id = Pubkey::new_unique();
        let config = MagazineConfig {
            targets: vec![
                PriceTarget::new(2.0, 5000).unwrap(),
                PriceTarget::new(3.0, 3000).unwrap(),
            ],
            program_id,
            validate_total_fraction: true,
            time_stop_secs: Some(DEFAULT_TIME_STOP_SECS),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_magazine_config_validate_disabled() {
        let program_id = Pubkey::new_unique();
        let config = MagazineConfig {
            targets: vec![
                PriceTarget::new(2.0, 5000).unwrap(),
                PriceTarget::new(3.0, 3000).unwrap(),
            ],
            program_id,
            validate_total_fraction: false,
            time_stop_secs: Some(DEFAULT_TIME_STOP_SECS),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_create_magazine_rejects_zero_entry_or_size() {
        let program_id = Pubkey::new_unique();
        let config = MagazineConfig::default_targets(program_id);
        let payer = Keypair::new();
        let rpc_client = RpcClient::new("http://localhost:8899".to_string());
        let mint = Pubkey::new_unique();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let size_zero = runtime.block_on(create_magazine_after_buy(
            &payer,
            mint,
            0,
            1_000,
            &config,
            &rpc_client,
        ));
        assert!(matches!(size_zero, Err(TriggerError::ConfigError(_))));

        let entry_zero = runtime.block_on(create_magazine_after_buy(
            &payer,
            mint,
            1_000,
            0,
            &config,
            &rpc_client,
        ));
        assert!(matches!(entry_zero, Err(TriggerError::ConfigError(_))));
    }

    #[test]
    fn test_create_virtual_magazine_uses_same_targets_without_tx_bytes() {
        let config = MagazineConfig::default_targets(Pubkey::new_unique());
        let bullets = create_virtual_magazine(1_000, &config).expect("virtual magazine");

        assert_eq!(bullets.len(), 3);
        assert!(bullets.iter().all(|bullet| bullet.tx_bytes.is_empty()));
        assert_eq!(bullets[0].target_price, 2_000);
        assert_eq!(bullets[0].position_fraction_bps, 2_500);
        assert_eq!(bullets[2].target_price, 5_000);
        assert_eq!(bullets[2].position_fraction_bps, 5_000);
    }

    #[tokio::test]
    async fn test_build_sell_transaction_rejects_unknown_program() {
        let payer = Keypair::new();
        let swap_plan = SwapPlanBuilder::new(payer.pubkey(), Pubkey::new_unique())
            .amount_in(1_000_000)
            .min_amount_out(1)
            .timeout_seconds(60)
            .build()
            .unwrap();

        let result = build_sell_transaction(
            &swap_plan,
            Pubkey::new_unique(),
            &payer,
            Hash::default(),
            Pubkey::new_unique(),
        )
        .await;

        assert!(matches!(result, Err(TriggerError::InvalidPool(_))));
    }
}
