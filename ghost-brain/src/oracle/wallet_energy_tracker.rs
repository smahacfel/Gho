//! WEST - Wallet Energy & State Tracker
//!
//! QMAN Part 1: Quantum-inspired wallet state tracking system that monitors
//! trader activity and maps it to quantum-like states.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │                  WEST - State Tracker                    │
//! │                                                          │
//! │  ┌────────────────┐      ┌─────────────────┐           │
//! │  │ Wallet Cache   │      │ Token Registry  │           │
//! │  │ (60s TTL)      │      │ (Dynamic List)  │           │
//! │  └────────────────┘      └─────────────────┘           │
//! │           │                       │                     │
//! │           └───────────┬───────────┘                     │
//! │                       ▼                                 │
//! │               ┌───────────────┐                         │
//! │               │ State Vector  │                         │
//! │               │    |ψ(t)⟩     │                         │
//! │               └───────────────┘                         │
//! └──────────────────────────────────────────────────────────┘
//!
//!  Input: PoolTransaction events (from Geyser Stream)
//!  Output: State vector |ψ(t)⟩ representing energy distribution
//! ```
//!
//! ## Concepts
//!
//! - **Energy**: `(SOL balance) × (Activity Score)` - wallets with more SOL have greater "mass"
//! - **States**: Wallets can be in "Free Liquidity" (holding SOL) or "Locked in Token X"
//! - **State Vector**: Represents the distribution of capital across different states

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Time-to-live for wallet cache entries (60 seconds)
const WALLET_TTL_MS: u64 = 60_000;

/// Represents a wallet's last known action type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// Wallet bought a token
    Buy,
    /// Wallet sold a token
    Sell,
    /// Wallet is holding (no recent activity)
    Hold,
}

/// Represents a wallet as a "particle" in the quantum-inspired model
///
/// Each wallet has:
/// - Energy (SOL balance × activity score)
/// - Current state (which token it's holding, or SOL if free)
/// - Last action (buy/sell/hold)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletParticle {
    /// Energy level: (SOL balance) * (Activity Score)
    /// Higher energy = more significant market participant
    pub energy: f32,

    /// Current token the wallet is holding
    /// None = wallet is in "Free Liquidity" state (holding SOL)
    pub current_token: Option<Pubkey>,

    /// Last observed action
    pub last_action: Action,

    /// Timestamp of last update (milliseconds since epoch)
    pub last_update_ms: u64,

    /// SOL balance estimate (from transaction volume observation)
    pub sol_balance_estimate: f64,

    /// Activity score: increases with transaction frequency
    /// Range: 0.1 (inactive) to 10.0 (very active)
    pub activity_score: f32,

    /// Number of transactions observed for this wallet
    pub tx_count: u32,
}

impl WalletParticle {
    /// Create a new wallet particle from a transaction event
    pub fn new(sol_volume: f64, token: Option<Pubkey>, action: Action, timestamp_ms: u64) -> Self {
        let activity_score = 1.0; // Initial activity score
        let sol_balance_estimate = sol_volume;
        let energy = (sol_balance_estimate as f32) * activity_score;

        Self {
            energy,
            current_token: token,
            last_action: action,
            last_update_ms: timestamp_ms,
            sol_balance_estimate,
            activity_score,
            tx_count: 1,
        }
    }

    /// Update the wallet particle with new transaction data
    pub fn update(
        &mut self,
        sol_volume: f64,
        token: Option<Pubkey>,
        action: Action,
        timestamp_ms: u64,
    ) {
        // Update action and token state
        self.last_action = action;
        self.current_token = token;
        self.last_update_ms = timestamp_ms;
        self.tx_count += 1;

        // Update SOL balance estimate (running average)
        self.sol_balance_estimate = (self.sol_balance_estimate * 0.7) + (sol_volume * 0.3);

        // Update activity score based on transaction frequency
        // More recent activity = higher score
        self.activity_score = (self.tx_count as f32).min(10.0).max(0.1);

        // Recalculate energy
        self.energy = (self.sol_balance_estimate as f32) * self.activity_score;
    }

    /// Check if this particle has expired (older than TTL)
    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        current_time_ms.saturating_sub(self.last_update_ms) > WALLET_TTL_MS
    }
}

/// Token in the registry - represents an observable quantum state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservedToken {
    /// Token mint address
    pub mint: Pubkey,

    /// Timestamp when token was first observed (milliseconds)
    pub first_seen_ms: u64,

    /// Total energy (capital) currently locked in this token
    pub total_energy: f64,

    /// Number of unique wallets holding this token
    pub holder_count: usize,

    /// Pool AMM ID associated with this token
    pub pool_amm_id: Pubkey,
}

/// State vector |ψ(t)⟩ - represents the quantum-like state of the market
///
/// This captures the distribution of capital (energy) across different states:
/// - Free liquidity (wallets holding SOL)
/// - Locked in various tokens
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateVector {
    /// Timestamp of this state snapshot (milliseconds)
    pub timestamp_ms: u64,

    /// Total free energy (SOL ready to trade)
    pub free_energy: f64,

    /// Energy locked per token (mint -> energy)
    pub token_energies: HashMap<Pubkey, f64>,

    /// Total number of active wallets tracked
    pub active_wallets: usize,

    /// Total energy in the system
    pub total_energy: f64,
}

impl StateVector {
    /// Create an empty state vector
    pub fn empty(timestamp_ms: u64) -> Self {
        Self {
            timestamp_ms,
            free_energy: 0.0,
            token_energies: HashMap::new(),
            active_wallets: 0,
            total_energy: 0.0,
        }
    }

    /// Calculate normalized probabilities for each state
    /// Returns a map of token -> probability (0.0-1.0)
    pub fn probabilities(&self) -> HashMap<Option<Pubkey>, f64> {
        let mut probs = HashMap::new();

        if self.total_energy > 0.0 {
            // Free liquidity probability
            probs.insert(None, self.free_energy / self.total_energy);

            // Token state probabilities
            for (mint, energy) in &self.token_energies {
                probs.insert(Some(*mint), energy / self.total_energy);
            }
        }

        probs
    }
}

/// Global Wallet Cache and Token Registry
///
/// This is the core state tracker that maintains:
/// - Active wallets (with 60s TTL)
/// - Observed tokens (dynamic registry)
/// - Current state vector
#[derive(Clone)]
pub struct WalletEnergyTracker {
    /// Cache of active wallet particles (wallet pubkey -> particle)
    /// Thread-safe with RwLock for concurrent read access
    wallet_cache: Arc<RwLock<HashMap<Pubkey, WalletParticle>>>,

    /// Registry of observed tokens (mint -> token info)
    token_registry: Arc<RwLock<HashMap<Pubkey, ObservedToken>>>,

    /// Current state vector |ψ(t)⟩
    state_vector: Arc<Mutex<StateVector>>,

    /// Last cleanup timestamp (for periodic cache cleanup)
    last_cleanup_ms: Arc<Mutex<u64>>,
}

impl WalletEnergyTracker {
    /// Create a new wallet energy tracker
    pub fn new() -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            wallet_cache: Arc::new(RwLock::new(HashMap::new())),
            token_registry: Arc::new(RwLock::new(HashMap::new())),
            state_vector: Arc::new(Mutex::new(StateVector::empty(now_ms))),
            last_cleanup_ms: Arc::new(Mutex::new(now_ms)),
        }
    }

    /// Process a pool transaction event and update wallet states
    ///
    /// This is the main entry point for real-time feed consumption.
    pub fn process_transaction(
        &self,
        pool_amm_id: Pubkey,
        wallet: Pubkey,
        token_mint: Pubkey,
        is_buy: bool,
        volume_sol: f64,
        timestamp_ms: u64,
    ) {
        // Determine action and token state
        let action = if is_buy { Action::Buy } else { Action::Sell };
        let current_token = if is_buy { Some(token_mint) } else { None };

        // Update wallet particle
        {
            let mut cache = self.wallet_cache.write();

            match cache.get_mut(&wallet) {
                Some(particle) => {
                    // Update existing particle
                    particle.update(volume_sol, current_token, action, timestamp_ms);
                }
                None => {
                    // Create new particle
                    let particle =
                        WalletParticle::new(volume_sol, current_token, action, timestamp_ms);
                    cache.insert(wallet, particle);
                }
            }
        }

        // Update token registry
        if is_buy {
            let mut registry = self.token_registry.write();

            match registry.get_mut(&token_mint) {
                Some(token) => {
                    // Token already observed, just update stats
                    token.holder_count += 1; // Will be recalculated in state vector update
                }
                None => {
                    // New token detected - add to registry
                    let token = ObservedToken {
                        mint: token_mint,
                        first_seen_ms: timestamp_ms,
                        total_energy: 0.0, // Will be calculated in state vector update
                        holder_count: 0,
                        pool_amm_id,
                    };
                    registry.insert(token_mint, token);
                }
            }
        }

        // Periodically cleanup expired wallets (every 10 seconds)
        self.cleanup_if_needed(timestamp_ms);

        // Update state vector
        self.update_state_vector(timestamp_ms);
    }

    /// Cleanup expired wallet particles (older than TTL)
    fn cleanup_if_needed(&self, current_time_ms: u64) {
        let mut last_cleanup = self.last_cleanup_ms.lock();

        // Only cleanup every 10 seconds to avoid overhead
        if current_time_ms.saturating_sub(*last_cleanup) < 10_000 {
            return;
        }

        *last_cleanup = current_time_ms;
        drop(last_cleanup);

        // Perform cleanup
        let mut cache = self.wallet_cache.write();
        cache.retain(|_, particle| !particle.is_expired(current_time_ms));
    }

    /// Update the state vector |ψ(t)⟩ based on current wallet states
    fn update_state_vector(&self, timestamp_ms: u64) {
        let cache = self.wallet_cache.read();
        let mut registry = self.token_registry.write();

        // Calculate energy distribution
        let mut free_energy = 0.0;
        let mut token_energies: HashMap<Pubkey, f64> = HashMap::new();
        let mut token_holders: HashMap<Pubkey, HashSet<Pubkey>> = HashMap::new();

        for (wallet, particle) in cache.iter() {
            match particle.current_token {
                Some(token_mint) => {
                    // Wallet is locked in a token
                    *token_energies.entry(token_mint).or_insert(0.0) += particle.energy as f64;
                    token_holders
                        .entry(token_mint)
                        .or_insert_with(HashSet::new)
                        .insert(*wallet);
                }
                None => {
                    // Wallet has free liquidity (SOL)
                    free_energy += particle.energy as f64;
                }
            }
        }

        // Update token registry with calculated stats
        for (mint, energy) in &token_energies {
            if let Some(token) = registry.get_mut(mint) {
                token.total_energy = *energy;
                token.holder_count = token_holders.get(mint).map(|s| s.len()).unwrap_or(0);
            }
        }

        let total_energy = free_energy + token_energies.values().sum::<f64>();

        // Update state vector
        let mut state = self.state_vector.lock();
        state.timestamp_ms = timestamp_ms;
        state.free_energy = free_energy;
        state.token_energies = token_energies;
        state.active_wallets = cache.len();
        state.total_energy = total_energy;
    }

    /// Get the current state vector |ψ(t)⟩
    pub fn get_state_vector(&self) -> StateVector {
        self.state_vector.lock().clone()
    }

    /// Get a snapshot of the wallet cache
    pub fn get_wallet_cache_snapshot(&self) -> HashMap<Pubkey, WalletParticle> {
        self.wallet_cache.read().clone()
    }

    /// Get a snapshot of the token registry
    pub fn get_token_registry_snapshot(&self) -> HashMap<Pubkey, ObservedToken> {
        self.token_registry.read().clone()
    }

    /// Get a specific wallet particle
    pub fn get_wallet(&self, wallet: &Pubkey) -> Option<WalletParticle> {
        self.wallet_cache.read().get(wallet).cloned()
    }

    /// Get a specific observed token
    pub fn get_token(&self, mint: &Pubkey) -> Option<ObservedToken> {
        self.token_registry.read().get(mint).cloned()
    }

    /// Get the list of currently observed tokens
    pub fn get_observed_tokens(&self) -> Vec<Pubkey> {
        self.token_registry.read().keys().copied().collect()
    }

    /// Get statistics about the tracker
    pub fn get_stats(&self) -> WestStats {
        let cache = self.wallet_cache.read();
        let registry = self.token_registry.read();
        let state = self.state_vector.lock();

        WestStats {
            active_wallets: cache.len(),
            observed_tokens: registry.len(),
            total_energy: state.total_energy,
            free_energy: state.free_energy,
            locked_energy: state.token_energies.values().sum::<f64>(),
        }
    }
}

impl Default for WalletEnergyTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the WEST tracker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WestStats {
    /// Number of active wallets being tracked
    pub active_wallets: usize,

    /// Number of tokens in the registry
    pub observed_tokens: usize,

    /// Total energy in the system
    pub total_energy: f64,

    /// Free energy (SOL ready to trade)
    pub free_energy: f64,

    /// Locked energy (in tokens)
    pub locked_energy: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wallet_particle_creation() {
        let wallet = WalletParticle::new(10.0, None, Action::Hold, 1000000);

        assert_eq!(wallet.sol_balance_estimate, 10.0);
        assert_eq!(wallet.activity_score, 1.0);
        assert_eq!(wallet.energy, 10.0);
        assert_eq!(wallet.tx_count, 1);
        assert_eq!(wallet.last_action, Action::Hold);
    }

    #[test]
    fn test_wallet_particle_update() {
        let mut wallet = WalletParticle::new(10.0, None, Action::Hold, 1000000);

        let token = Pubkey::new_unique();
        wallet.update(20.0, Some(token), Action::Buy, 2000000);

        assert_eq!(wallet.tx_count, 2);
        assert_eq!(wallet.last_action, Action::Buy);
        assert_eq!(wallet.current_token, Some(token));
        assert!(wallet.energy > 10.0); // Energy should increase
    }

    #[test]
    fn test_wallet_particle_expiry() {
        let wallet = WalletParticle::new(10.0, None, Action::Hold, 1000000);

        // Not expired within TTL
        assert!(!wallet.is_expired(1000000 + 30_000));

        // Expired after TTL
        assert!(wallet.is_expired(1000000 + 70_000));
    }

    #[test]
    fn test_tracker_creation() {
        let tracker = WalletEnergyTracker::new();
        let stats = tracker.get_stats();

        assert_eq!(stats.active_wallets, 0);
        assert_eq!(stats.observed_tokens, 0);
        assert_eq!(stats.total_energy, 0.0);
    }

    #[test]
    fn test_process_buy_transaction() {
        let tracker = WalletEnergyTracker::new();

        let pool = Pubkey::new_unique();
        let wallet = Pubkey::new_unique();
        let token = Pubkey::new_unique();

        // Process a buy transaction
        tracker.process_transaction(
            pool, wallet, token, true, // is_buy
            5.0,  // volume_sol
            1000000,
        );

        // Check wallet was added
        let wallet_particle = tracker.get_wallet(&wallet).unwrap();
        assert_eq!(wallet_particle.current_token, Some(token));
        assert_eq!(wallet_particle.last_action, Action::Buy);

        // Check token was added to registry
        let observed_token = tracker.get_token(&token).unwrap();
        assert_eq!(observed_token.mint, token);
        assert_eq!(observed_token.pool_amm_id, pool);

        // Check state vector
        let state = tracker.get_state_vector();
        assert_eq!(state.active_wallets, 1);
        assert!(state.token_energies.contains_key(&token));
        assert!(state.free_energy == 0.0); // All energy locked in token
    }

    #[test]
    fn test_process_sell_transaction() {
        let tracker = WalletEnergyTracker::new();

        let pool = Pubkey::new_unique();
        let wallet = Pubkey::new_unique();
        let token = Pubkey::new_unique();

        // First buy
        tracker.process_transaction(pool, wallet, token, true, 5.0, 1000000);

        // Then sell
        tracker.process_transaction(pool, wallet, token, false, 5.5, 2000000);

        // Check wallet state after sell
        let wallet_particle = tracker.get_wallet(&wallet).unwrap();
        assert_eq!(wallet_particle.current_token, None); // Back to free liquidity
        assert_eq!(wallet_particle.last_action, Action::Sell);

        // Check state vector - energy should be free now
        let state = tracker.get_state_vector();
        assert!(state.free_energy > 0.0);
    }

    #[test]
    fn test_state_vector_probabilities() {
        let tracker = WalletEnergyTracker::new();

        let pool = Pubkey::new_unique();
        let wallet1 = Pubkey::new_unique();
        let wallet2 = Pubkey::new_unique();
        let token = Pubkey::new_unique();

        // Wallet1 buys token
        tracker.process_transaction(pool, wallet1, token, true, 10.0, 1000000);

        // Wallet2 stays in free liquidity (sells)
        tracker.process_transaction(pool, wallet2, token, false, 10.0, 1000000);

        let state = tracker.get_state_vector();
        let probs = state.probabilities();

        // Should have roughly 50% free, 50% locked
        let free_prob = probs.get(&None).unwrap_or(&0.0);
        let token_prob = probs.get(&Some(token)).unwrap_or(&0.0);

        assert!(*free_prob > 0.0);
        assert!(*token_prob > 0.0);
        assert!((*free_prob + *token_prob - 1.0).abs() < 0.01); // Sum to ~1.0
    }

    #[test]
    fn test_multiple_tokens() {
        let tracker = WalletEnergyTracker::new();

        let pool1 = Pubkey::new_unique();
        let pool2 = Pubkey::new_unique();
        let wallet1 = Pubkey::new_unique();
        let wallet2 = Pubkey::new_unique();
        let token1 = Pubkey::new_unique();
        let token2 = Pubkey::new_unique();

        // Wallet1 buys token1
        tracker.process_transaction(pool1, wallet1, token1, true, 5.0, 1000000);

        // Wallet2 buys token2
        tracker.process_transaction(pool2, wallet2, token2, true, 10.0, 1000000);

        let state = tracker.get_state_vector();

        assert_eq!(state.token_energies.len(), 2);
        assert!(state.token_energies.contains_key(&token1));
        assert!(state.token_energies.contains_key(&token2));

        let tokens = tracker.get_observed_tokens();
        assert_eq!(tokens.len(), 2);
    }

    #[test]
    fn test_stats() {
        let tracker = WalletEnergyTracker::new();

        let pool = Pubkey::new_unique();
        let wallet1 = Pubkey::new_unique();
        let wallet2 = Pubkey::new_unique();
        let token = Pubkey::new_unique();

        // Add some transactions
        tracker.process_transaction(pool, wallet1, token, true, 5.0, 1000000);
        tracker.process_transaction(pool, wallet2, token, true, 10.0, 1000000);

        let stats = tracker.get_stats();

        assert_eq!(stats.active_wallets, 2);
        assert_eq!(stats.observed_tokens, 1);
        assert!(stats.total_energy > 0.0);
        assert!(stats.locked_energy > 0.0);
        assert_eq!(stats.free_energy, 0.0);
    }
}
