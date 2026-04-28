//! Portfolio Tracker Module
//!
//! Real-time monitoring of portfolio positions and P&L calculations.
//!
//! ## Features
//!
//! - Track tokens owned (mint/pubkey, amount)
//! - Track entry prices (average cost basis, position open time)
//! - Fetch current prices from Solana RPC/AMM pools
//! - Calculate P&L per position and total
//! - Periodic refresh (configurable, default 10s)
//! - Thread-safe state management with Arc<RwLock>
//!
//! ## Usage
//!
//! ```rust,no_run
//! use ghost_brain::portfolio::{PortfolioTracker, PortfolioConfig};
//! use solana_client::rpc_client::RpcClient;
//! use std::sync::Arc;
//!
//! let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
//! let config = PortfolioConfig {
//!     refresh_interval_secs: 10,
//!     authority_pubkey: "YOUR_WALLET_PUBKEY".to_string(),
//! };
//!
//! let tracker = PortfolioTracker::new(config, rpc_client);
//! tokio::spawn(async move {
//!     tracker.start().await.unwrap();
//! });
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::price_oracle::{PoolAddressCache, PriceOracle};

/// Portfolio tracker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioConfig {
    /// Refresh interval in seconds
    pub refresh_interval_secs: u64,

    /// Authority public key (wallet address)
    pub authority_pubkey: String,
}

impl Default for PortfolioConfig {
    fn default() -> Self {
        Self {
            refresh_interval_secs: 10,
            authority_pubkey: String::new(),
        }
    }
}

/// Token position in portfolio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPosition {
    /// Token mint address
    pub mint: Pubkey,

    /// Amount of tokens held (in smallest unit)
    pub amount: u64,

    /// Average entry price in lamports per token
    pub entry_price_lamports: u64,

    /// Current price in lamports per token (if available)
    pub current_price_lamports: Option<u64>,

    /// Total cost basis in lamports
    pub cost_basis_lamports: u64,

    /// Current value in lamports (if price available)
    pub current_value_lamports: Option<u64>,

    /// Profit/Loss in lamports
    pub pnl_lamports: i64,

    /// Profit/Loss percentage (0.0 - 1.0)
    pub pnl_percentage: f64,

    /// Timestamp when position was opened (Unix timestamp)
    pub opened_at: u64,

    /// Last price update timestamp
    pub last_price_update: u64,
}

impl TokenPosition {
    /// Create new token position
    pub fn new(mint: Pubkey, amount: u64, entry_price_lamports: u64) -> Self {
        let cost_basis_lamports =
            (amount as u128 * entry_price_lamports as u128 / 1_000_000_000) as u64;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            mint,
            amount,
            entry_price_lamports,
            current_price_lamports: None,
            cost_basis_lamports,
            current_value_lamports: None,
            pnl_lamports: 0,
            pnl_percentage: 0.0,
            opened_at: now,
            last_price_update: 0,
        }
    }

    /// Update current price and recalculate P&L
    pub fn update_price(&mut self, current_price_lamports: u64) {
        self.current_price_lamports = Some(current_price_lamports);

        // Calculate current value
        let current_value =
            (self.amount as u128 * current_price_lamports as u128 / 1_000_000_000) as u64;
        self.current_value_lamports = Some(current_value);

        // Calculate P&L
        self.pnl_lamports = current_value as i64 - self.cost_basis_lamports as i64;

        // Calculate P&L percentage
        if self.cost_basis_lamports > 0 {
            self.pnl_percentage = self.pnl_lamports as f64 / self.cost_basis_lamports as f64;
        }

        self.last_price_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }

    /// Add to position (average down/up)
    pub fn add_to_position(&mut self, amount: u64, price_lamports: u64) {
        let new_cost = (amount as u128 * price_lamports as u128 / 1_000_000_000) as u64;
        let total_cost = self.cost_basis_lamports + new_cost;
        let total_amount = self.amount + amount;

        // Calculate new average entry price
        if total_amount > 0 {
            self.entry_price_lamports =
                (total_cost as u128 * 1_000_000_000 / total_amount as u128) as u64;
        }

        self.amount = total_amount;
        self.cost_basis_lamports = total_cost;

        // Recalculate P&L if we have current price
        if let Some(current_price) = self.current_price_lamports {
            self.update_price(current_price);
        }
    }

    /// Remove from position (partial or full exit)
    pub fn remove_from_position(&mut self, amount: u64) -> Result<()> {
        if amount > self.amount {
            anyhow::bail!("Cannot remove more tokens than held");
        }

        // Reduce amount and cost basis proportionally
        let ratio = amount as f64 / self.amount as f64;
        self.amount -= amount;
        self.cost_basis_lamports = (self.cost_basis_lamports as f64 * (1.0 - ratio)) as u64;

        // Recalculate P&L if we have current price
        if let Some(current_price) = self.current_price_lamports {
            self.update_price(current_price);
        }

        Ok(())
    }
}

/// Portfolio state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioState {
    /// SOL balance in lamports
    pub sol_balance: u64,

    /// Map of token positions (mint -> position)
    pub positions: HashMap<Pubkey, TokenPosition>,

    /// Total portfolio value in lamports (SOL + all positions)
    pub total_value_lamports: u64,

    /// Total profit/loss in lamports
    pub total_pnl_lamports: i64,

    /// Total P&L percentage
    pub total_pnl_percentage: f64,

    /// Last update timestamp
    pub last_update: u64,
}

impl Default for PortfolioState {
    fn default() -> Self {
        Self {
            sol_balance: 0,
            positions: HashMap::new(),
            total_value_lamports: 0,
            total_pnl_lamports: 0,
            total_pnl_percentage: 0.0,
            last_update: 0,
        }
    }
}

impl PortfolioState {
    /// Recalculate total portfolio value and P&L
    pub fn recalculate_totals(&mut self) {
        let mut total_value = self.sol_balance;
        let mut total_cost = 0u64;
        let mut total_pnl = 0i64;

        for position in self.positions.values() {
            if let Some(current_value) = position.current_value_lamports {
                total_value += current_value;
            }
            total_cost += position.cost_basis_lamports;
            total_pnl += position.pnl_lamports;
        }

        self.total_value_lamports = total_value;
        self.total_pnl_lamports = total_pnl;

        // Calculate total P&L percentage
        if total_cost > 0 {
            self.total_pnl_percentage = total_pnl as f64 / total_cost as f64;
        }

        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }

    /// Add or update position
    pub fn add_position(&mut self, mint: Pubkey, amount: u64, price_lamports: u64) {
        if let Some(position) = self.positions.get_mut(&mint) {
            position.add_to_position(amount, price_lamports);
        } else {
            let position = TokenPosition::new(mint, amount, price_lamports);
            self.positions.insert(mint, position);
        }
        self.recalculate_totals();
    }

    /// Remove from position
    pub fn remove_position(&mut self, mint: &Pubkey, amount: u64) -> Result<()> {
        if let Some(position) = self.positions.get_mut(mint) {
            position.remove_from_position(amount)?;

            // Remove position if amount is zero
            if position.amount == 0 {
                self.positions.remove(mint);
            }
        }
        self.recalculate_totals();
        Ok(())
    }

    /// Update position price
    pub fn update_position_price(&mut self, mint: &Pubkey, current_price_lamports: u64) {
        if let Some(position) = self.positions.get_mut(mint) {
            position.update_price(current_price_lamports);
        }
        self.recalculate_totals();
    }
}

/// Portfolio tracker
pub struct PortfolioTracker {
    /// Configuration
    config: PortfolioConfig,

    /// Shared portfolio state
    state: Arc<RwLock<PortfolioState>>,

    /// RPC client for fetching on-chain data
    rpc_client: Arc<RpcClient>,

    /// Price oracle for fetching token prices
    price_oracle: Arc<PriceOracle>,

    /// Pool address cache
    pool_cache: Arc<PoolAddressCache>,

    /// Authority pubkey
    authority: Pubkey,
}

impl PortfolioTracker {
    /// Create new portfolio tracker
    pub fn new(config: PortfolioConfig, rpc_client: Arc<RpcClient>) -> Result<Self> {
        let authority =
            Pubkey::from_str(&config.authority_pubkey).context("Invalid authority pubkey")?;

        let price_oracle = Arc::new(PriceOracle::new(Arc::clone(&rpc_client)));
        let pool_cache = Arc::new(PoolAddressCache::new());

        Ok(Self {
            config,
            state: Arc::new(RwLock::new(PortfolioState::default())),
            rpc_client,
            price_oracle,
            pool_cache,
            authority,
        })
    }

    /// Get shared portfolio state
    pub fn get_state(&self) -> Arc<RwLock<PortfolioState>> {
        Arc::clone(&self.state)
    }

    /// Get current portfolio state (cloned)
    pub fn get_portfolio(&self) -> PortfolioState {
        self.state.read().unwrap().clone()
    }

    /// Start portfolio tracking loop
    pub async fn start(self: Arc<Self>) -> Result<()> {
        info!(
            "Starting portfolio tracker with {}s refresh interval",
            self.config.refresh_interval_secs
        );

        let mut interval = time::interval(Duration::from_secs(self.config.refresh_interval_secs));

        loop {
            interval.tick().await;

            if let Err(e) = self.refresh_portfolio().await {
                error!("Error refreshing portfolio: {}", e);
            }
        }
    }

    /// Refresh portfolio state
    async fn refresh_portfolio(&self) -> Result<()> {
        debug!("Refreshing portfolio state");

        // Update SOL balance
        self.update_sol_balance().await?;

        // Update token positions and prices
        self.update_token_positions().await?;

        info!("Portfolio refreshed successfully");
        Ok(())
    }

    /// Update SOL balance
    async fn update_sol_balance(&self) -> Result<()> {
        let authority = self.authority;
        let rpc_client = Arc::clone(&self.rpc_client);

        let balance = tokio::task::spawn_blocking(move || rpc_client.get_balance(&authority))
            .await
            .context("Task join error")?
            .context("Failed to fetch SOL balance")?;

        let mut state = self.state.write().unwrap();
        state.sol_balance = balance;
        debug!("Updated SOL balance: {} lamports", balance);

        Ok(())
    }

    /// Update token positions and fetch current prices
    async fn update_token_positions(&self) -> Result<()> {
        let positions: Vec<Pubkey> = {
            let state = self.state.read().unwrap();
            state.positions.keys().copied().collect()
        };

        for mint in positions {
            if let Err(e) = self.update_position_price(&mint).await {
                warn!("Failed to update price for {}: {}", mint, e);
            }
        }

        // Recalculate totals
        let mut state = self.state.write().unwrap();
        state.recalculate_totals();

        Ok(())
    }

    /// Update position price by fetching from AMM pool
    async fn update_position_price(&self, mint: &Pubkey) -> Result<()> {
        // Fetch current price from pool reserves
        let price = self.fetch_token_price(mint).await?;

        let mut state = self.state.write().unwrap();
        state.update_position_price(mint, price);
        debug!("Updated price for {}: {} lamports", mint, price);

        Ok(())
    }

    /// Fetch token price from AMM pool
    ///
    /// This method reads pool reserves and calculates the current price
    /// based on the constant product formula: price = reserve_quote / reserve_base
    async fn fetch_token_price(&self, mint: &Pubkey) -> Result<u64> {
        // Check if we have a cached pool address
        let pool_address = self.pool_cache.get(mint);

        // Fetch price using the price oracle
        let price = self
            .price_oracle
            .fetch_price(mint, pool_address.as_ref())
            .await
            .context(format!("Failed to fetch price for mint {}", mint))?;

        // If we got a price and didn't have a cached pool address,
        // the oracle found it, so we could cache it here if we had access to the pool address
        // For now, we rely on the oracle to handle caching internally

        Ok(price)
    }

    /// Record a buy transaction
    pub fn record_buy(&self, mint: Pubkey, amount: u64, price_lamports: u64) {
        let mut state = self.state.write().unwrap();
        state.add_position(mint, amount, price_lamports);
        info!(
            "Recorded buy: {} tokens of {} at {} lamports",
            amount, mint, price_lamports
        );
    }

    /// Record a sell transaction
    pub fn record_sell(&self, mint: &Pubkey, amount: u64) -> Result<()> {
        let mut state = self.state.write().unwrap();
        state.remove_position(mint, amount)?;
        info!("Recorded sell: {} tokens of {}", amount, mint);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_position_new() {
        let mint = Pubkey::new_unique();
        let amount = 1_000_000_000; // 1 token with 9 decimals
        let entry_price = 100_000_000; // 0.1 SOL

        let position = TokenPosition::new(mint, amount, entry_price);

        assert_eq!(position.mint, mint);
        assert_eq!(position.amount, amount);
        assert_eq!(position.entry_price_lamports, entry_price);
        assert_eq!(position.cost_basis_lamports, 100_000_000); // 0.1 SOL
    }

    #[test]
    fn test_token_position_update_price() {
        let mint = Pubkey::new_unique();
        let amount = 1_000_000_000; // 1 token
        let entry_price = 100_000_000; // 0.1 SOL

        let mut position = TokenPosition::new(mint, amount, entry_price);

        // Price goes up to 0.2 SOL
        position.update_price(200_000_000);

        assert_eq!(position.current_price_lamports, Some(200_000_000));
        assert_eq!(position.current_value_lamports, Some(200_000_000));
        assert_eq!(position.pnl_lamports, 100_000_000); // +0.1 SOL profit
        assert!((position.pnl_percentage - 1.0).abs() < 0.001); // 100% gain
    }

    #[test]
    fn test_token_position_add_to_position() {
        let mint = Pubkey::new_unique();
        let mut position = TokenPosition::new(mint, 1_000_000_000, 100_000_000);

        // Buy more at higher price
        position.add_to_position(1_000_000_000, 200_000_000);

        assert_eq!(position.amount, 2_000_000_000);
        assert_eq!(position.entry_price_lamports, 150_000_000); // Average: 0.15 SOL
        assert_eq!(position.cost_basis_lamports, 300_000_000); // 0.3 SOL total
    }

    #[test]
    fn test_portfolio_state_add_position() {
        let mut state = PortfolioState::default();
        let mint = Pubkey::new_unique();

        state.add_position(mint, 1_000_000_000, 100_000_000);

        assert_eq!(state.positions.len(), 1);
        assert_eq!(state.positions.get(&mint).unwrap().amount, 1_000_000_000);
    }

    #[test]
    fn test_portfolio_state_recalculate_totals() {
        let mut state = PortfolioState::default();
        state.sol_balance = 1_000_000_000; // 1 SOL

        let mint1 = Pubkey::new_unique();
        let mint2 = Pubkey::new_unique();

        // Add two positions
        state.add_position(mint1, 1_000_000_000, 100_000_000); // Cost: 0.1 SOL
        state.add_position(mint2, 2_000_000_000, 50_000_000); // Cost: 0.1 SOL

        // Update prices (both 2x gains)
        state.update_position_price(&mint1, 200_000_000);
        state.update_position_price(&mint2, 100_000_000);

        assert_eq!(state.total_value_lamports, 1_400_000_000); // 1 SOL + 0.2 + 0.2
        assert_eq!(state.total_pnl_lamports, 200_000_000); // +0.2 SOL profit
        assert!((state.total_pnl_percentage - 1.0).abs() < 0.001); // 100% gain
    }
}
