//! Flowfield Construction & Extraction Module (WHF Part 1)
//!
//! This module builds dynamic market flow fields from transaction data aggregated
//! from Geyser Stream/WebSocket feeds. It provides per-slot and per-wallet aggregation
//! with rolling window support for temporal analysis.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │         Flowfield Construction (WHF Part 1)             │
//! │                                                         │
//! │  Input: PoolTransaction Stream                         │
//! │         (slot, wallet, buy/sell, volume)                │
//! │                                                         │
//! │  ┌──────────────┐   ┌──────────────┐   ┌─────────────┐│
//! │  │ Slot-based   │   │ Wallet-based │   │ Rolling     ││
//! │  │ Aggregation  │   │ Aggregation  │   │ Window      ││
//! │  └──────────────┘   └──────────────┘   └─────────────┘│
//! │                                                         │
//! │  Output: FlowVector Arrays F(t)                        │
//! │          { buy, sell, wallets, net }                    │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Concepts
//!
//! - **FlowVector**: Represents aggregated buy/sell activity for a time period
//! - **Slot Aggregation**: Groups transactions by Solana slot number
//! - **Wallet Aggregation**: Groups transactions by wallet address
//! - **Rolling Window**: Maintains temporal data for analysis (20-60s default)
//!
//! ## Integration Points
//!
//! - **Input**: `PoolTransaction` events from Seer/Geyser Stream
//! - **Output**: Flow vectors consumed by WHF Part 2 (Field Analysis)
//! - **WEST Integration**: Compatible with WalletEnergyTracker for quantum state mapping

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, VecDeque};
use std::str::FromStr;

/// Default rolling window duration in milliseconds (30 seconds)
pub const DEFAULT_WINDOW_MS: u64 = 30_000;

/// Minimum window duration (20 seconds)
pub const MIN_WINDOW_MS: u64 = 20_000;

/// Maximum window duration (60 seconds)
pub const MAX_WINDOW_MS: u64 = 60_000;

/// Represents aggregated buy/sell flow for a time period
///
/// This structure captures the market flow dynamics by aggregating
/// transaction volumes and participant counts.
///
/// Note: Uses f32 for volume fields for memory efficiency. This provides
/// ~7 decimal digits of precision, which is sufficient for SOL volumes
/// (typical range: 0.001 to 10,000 SOL). For applications requiring higher
/// precision, consider using f64 in FlowTransaction and converting at aggregation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlowVector {
    /// Total buy volume (SOL)
    pub buy: f32,

    /// Total sell volume (SOL)
    pub sell: f32,

    /// Number of unique wallets participating
    pub wallets: usize,

    /// Net flow: buy - sell (positive = accumulation, negative = distribution)
    pub net: f32,
}

impl FlowVector {
    /// Create a new empty flow vector
    pub fn new() -> Self {
        Self {
            buy: 0.0,
            sell: 0.0,
            wallets: 0,
            net: 0.0,
        }
    }

    /// Create a flow vector from individual components
    pub fn from_components(buy: f32, sell: f32, wallets: usize) -> Self {
        Self {
            buy,
            sell,
            wallets,
            net: buy - sell,
        }
    }

    /// Add a transaction to this flow vector
    fn add_transaction(&mut self, is_buy: bool, volume: f32) {
        if is_buy {
            self.buy += volume;
        } else {
            self.sell += volume;
        }
        self.net = self.buy - self.sell;
    }

    /// Merge another flow vector into this one
    ///
    /// Note: Wallet count uses max() as a conservative heuristic since we don't track
    /// the actual wallet set in FlowVector (for memory efficiency). This may underestimate
    /// unique wallets when merging flows with overlapping wallets. For precise wallet
    /// counting, use FlowfieldExtractor's get_aggregate_flow() which tracks the actual set.
    pub fn merge(&mut self, other: &FlowVector) {
        self.buy += other.buy;
        self.sell += other.sell;
        self.wallets = self.wallets.max(other.wallets); // Conservative estimate
        self.net = self.buy - self.sell;
    }

    /// Get the dominant flow direction
    pub fn flow_direction(&self) -> FlowDirection {
        if self.net > 0.0 {
            FlowDirection::Accumulation
        } else if self.net < 0.0 {
            FlowDirection::Distribution
        } else {
            FlowDirection::Neutral
        }
    }

    /// Calculate buy/sell ratio (returns None if sell is zero)
    pub fn buy_sell_ratio(&self) -> Option<f32> {
        if self.sell > 0.0 {
            Some(self.buy / self.sell)
        } else {
            None
        }
    }
}

impl Default for FlowVector {
    fn default() -> Self {
        Self::new()
    }
}

/// Direction of net capital flow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowDirection {
    /// Net buying pressure (accumulation)
    Accumulation,
    /// Net selling pressure (distribution)
    Distribution,
    /// Balanced flow
    Neutral,
}

/// Represents a transaction event for flowfield construction
///
/// This is a simplified view of PoolTransaction focused on flow analysis
#[derive(Debug, Clone)]
pub struct FlowTransaction {
    /// Solana slot number
    pub slot: u64,

    /// Wallet/signer address
    pub wallet: Pubkey,

    /// True if buy, false if sell
    pub is_buy: bool,

    /// Volume in SOL
    pub volume_sol: f32,

    /// Timestamp in milliseconds since epoch
    pub timestamp_ms: u64,
}

/// Aggregated flow data for a specific slot
#[derive(Debug, Clone)]
struct SlotFlow {
    /// Slot number
    slot: u64,

    /// Flow vector for this slot
    flow: FlowVector,

    /// Set of unique wallets in this slot
    wallets: Vec<Pubkey>,

    /// Timestamp of first transaction in this slot
    timestamp_ms: u64,
}

/// Aggregated flow data for a specific wallet
#[derive(Debug, Clone)]
struct WalletFlow {
    /// Wallet address
    wallet: Pubkey,

    /// Flow vector for this wallet
    flow: FlowVector,

    /// Timestamp of last activity
    last_timestamp_ms: u64,
}

/// Configuration for flowfield extractor
#[derive(Debug, Clone)]
pub struct FlowfieldConfig {
    /// Rolling window duration in milliseconds
    pub window_ms: u64,

    /// Whether to track per-slot aggregation
    pub enable_slot_aggregation: bool,

    /// Whether to track per-wallet aggregation
    pub enable_wallet_aggregation: bool,
}

impl Default for FlowfieldConfig {
    fn default() -> Self {
        Self {
            window_ms: DEFAULT_WINDOW_MS,
            enable_slot_aggregation: true,
            enable_wallet_aggregation: true,
        }
    }
}

impl FlowfieldConfig {
    /// Create a new config with custom window size
    pub fn with_window(window_ms: u64) -> Self {
        let clamped_window = window_ms.clamp(MIN_WINDOW_MS, MAX_WINDOW_MS);
        Self {
            window_ms: clamped_window,
            ..Default::default()
        }
    }
}

/// Main flowfield extraction engine
///
/// Aggregates transaction streams into flow vectors with rolling window support.
pub struct FlowfieldExtractor {
    /// Configuration
    config: FlowfieldConfig,

    /// Slot-based aggregation (slot -> SlotFlow)
    slot_flows: HashMap<u64, SlotFlow>,

    /// Wallet-based aggregation (wallet -> WalletFlow)
    wallet_flows: HashMap<Pubkey, WalletFlow>,

    /// Transaction history for rolling window (timestamp-ordered)
    transaction_window: VecDeque<FlowTransaction>,

    /// Current window start time
    window_start_ms: u64,
}

impl FlowfieldExtractor {
    /// Create a new flowfield extractor with default configuration
    pub fn new() -> Self {
        Self::with_config(FlowfieldConfig::default())
    }

    /// Create a new flowfield extractor with custom configuration
    pub fn with_config(config: FlowfieldConfig) -> Self {
        Self {
            config,
            slot_flows: HashMap::new(),
            wallet_flows: HashMap::new(),
            transaction_window: VecDeque::new(),
            window_start_ms: 0,
        }
    }

    /// Process a transaction and update flow fields
    ///
    /// # Arguments
    ///
    /// * `transaction` - Transaction to process
    ///
    /// # Returns
    ///
    /// True if the transaction was accepted (within window), false if rejected (too old)
    pub fn process_transaction(&mut self, transaction: FlowTransaction) -> bool {
        // Initialize window on first transaction
        if self.window_start_ms == 0 {
            self.window_start_ms = transaction.timestamp_ms;
        }

        // Reject transactions outside the rolling window
        if transaction.timestamp_ms < self.window_start_ms {
            return false;
        }

        // Update slot aggregation
        if self.config.enable_slot_aggregation {
            self.update_slot_flow(&transaction);
        }

        // Update wallet aggregation
        if self.config.enable_wallet_aggregation {
            self.update_wallet_flow(&transaction);
        }

        // Add to transaction window
        self.transaction_window.push_back(transaction.clone());

        // Update rolling window
        self.update_window(transaction.timestamp_ms);

        true
    }

    /// Update slot-based flow aggregation
    fn update_slot_flow(&mut self, tx: &FlowTransaction) {
        let slot_flow = self.slot_flows.entry(tx.slot).or_insert_with(|| SlotFlow {
            slot: tx.slot,
            flow: FlowVector::new(),
            wallets: Vec::new(),
            timestamp_ms: tx.timestamp_ms,
        });

        // Update flow vector
        slot_flow.flow.add_transaction(tx.is_buy, tx.volume_sol);

        // Track unique wallets
        if !slot_flow.wallets.contains(&tx.wallet) {
            slot_flow.wallets.push(tx.wallet);
            slot_flow.flow.wallets = slot_flow.wallets.len();
        }
    }

    /// Update wallet-based flow aggregation
    fn update_wallet_flow(&mut self, tx: &FlowTransaction) {
        let wallet_flow = self
            .wallet_flows
            .entry(tx.wallet)
            .or_insert_with(|| WalletFlow {
                wallet: tx.wallet,
                flow: FlowVector::new(),
                last_timestamp_ms: tx.timestamp_ms,
            });

        // Update flow vector
        wallet_flow.flow.add_transaction(tx.is_buy, tx.volume_sol);
        wallet_flow.flow.wallets = 1; // Each wallet is counted as 1
        wallet_flow.last_timestamp_ms = tx.timestamp_ms;
    }

    /// Update rolling window and evict old data
    fn update_window(&mut self, current_time_ms: u64) {
        let window_cutoff = current_time_ms.saturating_sub(self.config.window_ms);

        // Remove old transactions from window
        while let Some(tx) = self.transaction_window.front() {
            if tx.timestamp_ms < window_cutoff {
                self.transaction_window.pop_front();
            } else {
                break;
            }
        }

        // Remove old slot flows
        if self.config.enable_slot_aggregation {
            self.slot_flows
                .retain(|_, slot_flow| slot_flow.timestamp_ms >= window_cutoff);
        }

        // Remove old wallet flows
        if self.config.enable_wallet_aggregation {
            self.wallet_flows
                .retain(|_, wallet_flow| wallet_flow.last_timestamp_ms >= window_cutoff);
        }

        // Update window start time
        self.window_start_ms = window_cutoff;
    }

    /// Get flow vector for a specific slot
    ///
    /// Returns None if slot is not found or slot aggregation is disabled
    pub fn get_slot_flow(&self, slot: u64) -> Option<FlowVector> {
        self.slot_flows.get(&slot).map(|sf| sf.flow.clone())
    }

    /// Get flow vector for a specific wallet
    ///
    /// Returns None if wallet is not found or wallet aggregation is disabled
    pub fn get_wallet_flow(&self, wallet: &Pubkey) -> Option<FlowVector> {
        self.wallet_flows.get(wallet).map(|wf| wf.flow.clone())
    }

    /// Get all slot flows currently in the window
    ///
    /// Returns a vector of (slot, FlowVector) pairs sorted by slot number
    pub fn get_all_slot_flows(&self) -> Vec<(u64, FlowVector)> {
        let mut flows: Vec<_> = self
            .slot_flows
            .iter()
            .map(|(slot, sf)| (*slot, sf.flow.clone()))
            .collect();
        flows.sort_by_key(|(slot, _)| *slot);
        flows
    }

    /// Get all wallet flows currently in the window
    ///
    /// Returns a vector of (Pubkey, FlowVector) pairs
    pub fn get_all_wallet_flows(&self) -> Vec<(Pubkey, FlowVector)> {
        self.wallet_flows
            .iter()
            .map(|(wallet, wf)| (*wallet, wf.flow.clone()))
            .collect()
    }

    /// Get aggregate flow vector for the entire current window
    ///
    /// This sums all activity across all slots and wallets in the current window
    pub fn get_aggregate_flow(&self) -> FlowVector {
        let mut aggregate = FlowVector::new();
        let mut unique_wallets = std::collections::HashSet::new();

        for tx in &self.transaction_window {
            aggregate.add_transaction(tx.is_buy, tx.volume_sol);
            unique_wallets.insert(tx.wallet);
        }

        aggregate.wallets = unique_wallets.len();
        aggregate
    }

    /// Get the number of transactions in the current window
    pub fn window_transaction_count(&self) -> usize {
        self.transaction_window.len()
    }

    /// Get the number of unique slots in the current window
    pub fn window_slot_count(&self) -> usize {
        self.slot_flows.len()
    }

    /// Get the number of unique wallets in the current window
    pub fn window_wallet_count(&self) -> usize {
        self.wallet_flows.len()
    }

    /// Clear all accumulated data
    pub fn clear(&mut self) {
        self.slot_flows.clear();
        self.wallet_flows.clear();
        self.transaction_window.clear();
        self.window_start_ms = 0;
    }
}

impl Default for FlowfieldExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to create FlowTransaction from PoolTransaction data
///
/// This bridges the gap between the PoolTransaction event structure
/// and the FlowTransaction structure used by flowfield extraction.
///
/// Note: Accepts f64 for volume_sol to match PoolTransaction.volume_sol type,
/// then converts to f32 for FlowVector aggregation (sufficient precision for SOL volumes).
pub fn flow_transaction_from_pool_event(
    slot: u64,
    wallet_str: &str,
    is_buy: bool,
    volume_sol: f64,
    timestamp_ms: u64,
) -> Result<FlowTransaction, String> {
    let wallet =
        Pubkey::from_str(wallet_str).map_err(|e| format!("Invalid wallet pubkey: {}", e))?;

    Ok(FlowTransaction {
        slot,
        wallet,
        is_buy,
        volume_sol: volume_sol as f32, // Convert to f32 for memory efficiency
        timestamp_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_vector_creation() {
        let fv = FlowVector::new();
        assert_eq!(fv.buy, 0.0);
        assert_eq!(fv.sell, 0.0);
        assert_eq!(fv.wallets, 0);
        assert_eq!(fv.net, 0.0);

        let fv2 = FlowVector::from_components(100.0, 50.0, 5);
        assert_eq!(fv2.buy, 100.0);
        assert_eq!(fv2.sell, 50.0);
        assert_eq!(fv2.wallets, 5);
        assert_eq!(fv2.net, 50.0);
    }

    #[test]
    fn test_flow_vector_add_transaction() {
        let mut fv = FlowVector::new();
        fv.add_transaction(true, 10.0);
        assert_eq!(fv.buy, 10.0);
        assert_eq!(fv.net, 10.0);

        fv.add_transaction(false, 5.0);
        assert_eq!(fv.sell, 5.0);
        assert_eq!(fv.net, 5.0);
    }

    #[test]
    fn test_flow_vector_merge() {
        let mut fv1 = FlowVector::from_components(10.0, 5.0, 2);
        let fv2 = FlowVector::from_components(20.0, 10.0, 3);

        fv1.merge(&fv2);
        assert_eq!(fv1.buy, 30.0);
        assert_eq!(fv1.sell, 15.0);
        assert_eq!(fv1.wallets, 3); // Takes max
        assert_eq!(fv1.net, 15.0);
    }

    #[test]
    fn test_flow_direction() {
        let fv_acc = FlowVector::from_components(100.0, 50.0, 5);
        assert_eq!(fv_acc.flow_direction(), FlowDirection::Accumulation);

        let fv_dist = FlowVector::from_components(50.0, 100.0, 5);
        assert_eq!(fv_dist.flow_direction(), FlowDirection::Distribution);

        let fv_neutral = FlowVector::from_components(50.0, 50.0, 5);
        assert_eq!(fv_neutral.flow_direction(), FlowDirection::Neutral);
    }

    #[test]
    fn test_buy_sell_ratio() {
        let fv = FlowVector::from_components(100.0, 50.0, 5);
        assert_eq!(fv.buy_sell_ratio(), Some(2.0));

        let fv_zero_sell = FlowVector::from_components(100.0, 0.0, 5);
        assert_eq!(fv_zero_sell.buy_sell_ratio(), None);
    }

    #[test]
    fn test_flowfield_extractor_basic() {
        let mut extractor = FlowfieldExtractor::new();

        let wallet1 = Pubkey::new_unique();
        let wallet2 = Pubkey::new_unique();

        let tx1 = FlowTransaction {
            slot: 1000,
            wallet: wallet1,
            is_buy: true,
            volume_sol: 10.0,
            timestamp_ms: 1000,
        };

        let tx2 = FlowTransaction {
            slot: 1000,
            wallet: wallet2,
            is_buy: false,
            volume_sol: 5.0,
            timestamp_ms: 1100,
        };

        assert!(extractor.process_transaction(tx1));
        assert!(extractor.process_transaction(tx2));

        assert_eq!(extractor.window_transaction_count(), 2);
        assert_eq!(extractor.window_slot_count(), 1);
        assert_eq!(extractor.window_wallet_count(), 2);
    }

    #[test]
    fn test_slot_aggregation() {
        let mut extractor = FlowfieldExtractor::new();
        let wallet1 = Pubkey::new_unique();
        let wallet2 = Pubkey::new_unique();

        // Two transactions in same slot
        extractor.process_transaction(FlowTransaction {
            slot: 1000,
            wallet: wallet1,
            is_buy: true,
            volume_sol: 10.0,
            timestamp_ms: 1000,
        });

        extractor.process_transaction(FlowTransaction {
            slot: 1000,
            wallet: wallet2,
            is_buy: false,
            volume_sol: 5.0,
            timestamp_ms: 1100,
        });

        let slot_flow = extractor.get_slot_flow(1000).unwrap();
        assert_eq!(slot_flow.buy, 10.0);
        assert_eq!(slot_flow.sell, 5.0);
        assert_eq!(slot_flow.wallets, 2);
        assert_eq!(slot_flow.net, 5.0);
    }

    #[test]
    fn test_wallet_aggregation() {
        let mut extractor = FlowfieldExtractor::new();
        let wallet = Pubkey::new_unique();

        // Multiple transactions from same wallet
        extractor.process_transaction(FlowTransaction {
            slot: 1000,
            wallet,
            is_buy: true,
            volume_sol: 10.0,
            timestamp_ms: 1000,
        });

        extractor.process_transaction(FlowTransaction {
            slot: 1001,
            wallet,
            is_buy: true,
            volume_sol: 20.0,
            timestamp_ms: 1500,
        });

        let wallet_flow = extractor.get_wallet_flow(&wallet).unwrap();
        assert_eq!(wallet_flow.buy, 30.0);
        assert_eq!(wallet_flow.sell, 0.0);
        assert_eq!(wallet_flow.wallets, 1);
        assert_eq!(wallet_flow.net, 30.0);
    }

    #[test]
    fn test_rolling_window_eviction() {
        let config = FlowfieldConfig::with_window(1000); // 1 second window
        let mut extractor = FlowfieldExtractor::with_config(config);

        let wallet = Pubkey::new_unique();

        // Add transaction at t=1000
        extractor.process_transaction(FlowTransaction {
            slot: 1000,
            wallet,
            is_buy: true,
            volume_sol: 10.0,
            timestamp_ms: 1000,
        });

        assert_eq!(extractor.window_transaction_count(), 1);

        // Add transaction at t=2500 (1.5s later, old one should be evicted)
        extractor.process_transaction(FlowTransaction {
            slot: 1002,
            wallet,
            is_buy: true,
            volume_sol: 20.0,
            timestamp_ms: 2500,
        });

        // First transaction should be evicted
        assert_eq!(extractor.window_transaction_count(), 1);
        assert!(extractor.get_slot_flow(1000).is_none());
    }

    #[test]
    fn test_aggregate_flow() {
        let mut extractor = FlowfieldExtractor::new();

        let wallet1 = Pubkey::new_unique();
        let wallet2 = Pubkey::new_unique();

        extractor.process_transaction(FlowTransaction {
            slot: 1000,
            wallet: wallet1,
            is_buy: true,
            volume_sol: 10.0,
            timestamp_ms: 1000,
        });

        extractor.process_transaction(FlowTransaction {
            slot: 1001,
            wallet: wallet2,
            is_buy: false,
            volume_sol: 5.0,
            timestamp_ms: 1100,
        });

        let agg = extractor.get_aggregate_flow();
        assert_eq!(agg.buy, 10.0);
        assert_eq!(agg.sell, 5.0);
        assert_eq!(agg.wallets, 2);
        assert_eq!(agg.net, 5.0);
    }

    #[test]
    fn test_get_all_slot_flows() {
        let mut extractor = FlowfieldExtractor::new();
        let wallet = Pubkey::new_unique();

        extractor.process_transaction(FlowTransaction {
            slot: 1000,
            wallet,
            is_buy: true,
            volume_sol: 10.0,
            timestamp_ms: 1000,
        });

        extractor.process_transaction(FlowTransaction {
            slot: 1001,
            wallet,
            is_buy: false,
            volume_sol: 5.0,
            timestamp_ms: 1100,
        });

        let all_flows = extractor.get_all_slot_flows();
        assert_eq!(all_flows.len(), 2);
        assert_eq!(all_flows[0].0, 1000);
        assert_eq!(all_flows[1].0, 1001);
    }

    #[test]
    fn test_config_window_clamping() {
        let config = FlowfieldConfig::with_window(10_000); // 10s (below min)
        assert_eq!(config.window_ms, MIN_WINDOW_MS);

        let config2 = FlowfieldConfig::with_window(100_000); // 100s (above max)
        assert_eq!(config2.window_ms, MAX_WINDOW_MS);

        let config3 = FlowfieldConfig::with_window(30_000); // 30s (valid)
        assert_eq!(config3.window_ms, 30_000);
    }

    #[test]
    fn test_flow_transaction_from_pool_event() {
        let wallet = Pubkey::new_unique();
        let wallet_str = wallet.to_string();

        let result = flow_transaction_from_pool_event(1000, &wallet_str, true, 10.5, 2000);

        assert!(result.is_ok());
        let tx = result.unwrap();
        assert_eq!(tx.slot, 1000);
        assert_eq!(tx.wallet, wallet);
        assert_eq!(tx.is_buy, true);
        assert_eq!(tx.volume_sol, 10.5);
        assert_eq!(tx.timestamp_ms, 2000);
    }

    #[test]
    fn test_flow_transaction_from_pool_event_invalid_wallet() {
        let result = flow_transaction_from_pool_event(1000, "invalid_pubkey", true, 10.0, 2000);

        assert!(result.is_err());
    }
}
