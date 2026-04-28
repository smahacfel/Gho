//! Portfolio Bridge Module
//!
//! Bridges the portfolio tracker with the GUI backend state.
//! Periodically syncs portfolio data to the AppState for API and WebSocket updates.

use crate::portfolio::PortfolioState as TrackerPortfolioState;
use crate::state::{AppState, Portfolio, Position};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info};

/// Portfolio bridge configuration
pub struct PortfolioBridgeConfig {
    /// Sync interval in seconds
    pub sync_interval_secs: u64,
}

impl Default for PortfolioBridgeConfig {
    fn default() -> Self {
        Self {
            sync_interval_secs: 5, // Sync every 5 seconds
        }
    }
}

/// Portfolio bridge
///
/// Syncs portfolio data from the tracker to the GUI backend state
pub struct PortfolioBridge {
    config: PortfolioBridgeConfig,
    #[allow(dead_code)]
    app_state: Arc<AppState>,
}

impl PortfolioBridge {
    /// Create new portfolio bridge
    pub fn new(config: PortfolioBridgeConfig, app_state: Arc<AppState>) -> Self {
        Self { config, app_state }
    }

    /// Start the bridge sync loop
    pub async fn start(self) {
        info!(
            "Starting portfolio bridge with {}s sync interval",
            self.config.sync_interval_secs
        );

        let mut sync_interval = interval(Duration::from_secs(self.config.sync_interval_secs));

        loop {
            sync_interval.tick().await;

            if let Err(e) = self.sync_portfolio().await {
                error!("Error syncing portfolio: {}", e);
            }
        }
    }

    /// Sync portfolio from tracker to app state
    async fn sync_portfolio(&self) -> anyhow::Result<()> {
        // TODO: Get portfolio from tracker once integrated
        // For now, this is a placeholder that updates with dummy data

        debug!("Syncing portfolio to GUI backend state");

        // This will be replaced with actual tracker data:
        // let tracker_state = portfolio_tracker.get_portfolio();
        // let gui_portfolio = convert_to_gui_portfolio(tracker_state);
        // self.app_state.update_portfolio(gui_portfolio);

        Ok(())
    }
}

/// Convert portfolio tracker state to GUI portfolio format
#[allow(dead_code)]
fn convert_to_gui_portfolio(tracker_state: TrackerPortfolioState) -> Portfolio {
    let positions: Vec<Position> = tracker_state
        .positions
        .values()
        .map(|pos| Position {
            mint: pos.mint.to_string(),
            amount: pos.amount,
            entry_price: pos.entry_price_lamports,
            current_price: pos.current_price_lamports,
            pnl: pos.pnl_lamports,
            opened_at: pos.opened_at,
        })
        .collect();

    Portfolio {
        sol_balance: tracker_state.sol_balance,
        positions,
        total_value: tracker_state.total_value_lamports,
        total_pnl: tracker_state.total_pnl_lamports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::TokenPosition;
    use solana_sdk::pubkey::Pubkey;
    use std::collections::HashMap;

    #[test]
    fn test_convert_to_gui_portfolio() {
        let mut positions = HashMap::new();
        let mint = Pubkey::new_unique();

        let token_pos = TokenPosition {
            mint,
            amount: 1_000_000_000,
            entry_price_lamports: 100_000_000,
            current_price_lamports: Some(150_000_000),
            cost_basis_lamports: 100_000_000,
            current_value_lamports: Some(150_000_000),
            pnl_lamports: 50_000_000,
            pnl_percentage: 0.5,
            opened_at: 1234567890,
            last_price_update: 1234567900,
        };

        positions.insert(mint, token_pos);

        let tracker_state = TrackerPortfolioState {
            sol_balance: 1_000_000_000,
            positions,
            total_value_lamports: 1_150_000_000,
            total_pnl_lamports: 50_000_000,
            total_pnl_percentage: 0.05,
            last_update: 1234567900,
        };

        let gui_portfolio = convert_to_gui_portfolio(tracker_state);

        assert_eq!(gui_portfolio.sol_balance, 1_000_000_000);
        assert_eq!(gui_portfolio.positions.len(), 1);
        assert_eq!(gui_portfolio.total_value, 1_150_000_000);
        assert_eq!(gui_portfolio.total_pnl, 50_000_000);

        let pos = &gui_portfolio.positions[0];
        assert_eq!(pos.mint, mint.to_string());
        assert_eq!(pos.amount, 1_000_000_000);
        assert_eq!(pos.entry_price, 100_000_000);
        assert_eq!(pos.current_price, Some(150_000_000));
        assert_eq!(pos.pnl, 50_000_000);
    }
}
