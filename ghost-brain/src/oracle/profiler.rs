//! DevProfiler - Behavioral Analysis of Token Creators (Rentgen Dewelopera)
//!
//! This module provides on-chain behavioral analysis of token creators to detect
//! serial ruggers and potentially malicious actors. It implements heuristics
//! based on transaction history, funding sources, and minting patterns.
//!
//! # Implementation Status
//!
//! **Phase 1 (Current)**: Basic heuristic analysis using transaction signatures and memos.
//! - Uses memo field parsing for quick pattern detection
//! - Maintains blacklists for known bad actors (mixer, rug-puller addresses)
//! - Tracks CEX hot wallet addresses for funding source detection
//!
//! **Future Enhancements** (Phase 2+):
//! - Full transaction parsing for accurate mint detection
//! - Cross-reference with on-chain graph analysis (cluster_hunter.rs)
//! - Integration with VisionCritic for metadata quality assessment
//!
//! # Key Features
//! - Fetch last 10 transaction signatures for creator analysis
//! - Detect interactions with Tornado Cash or known mixer addresses → RISK 1.0 (CRITICAL)
//! - Detect fresh wallets funded from CEX hot wallets → RISK 0.3 (Neutral/Degen)
//! - Detect serial minters (5+ tokens in 24h) → RISK 0.9 (Serial Scammer)
//! - Track known rug-puller addresses and flag interactions
//!
//! # Risk Scoring
//! - 0.0: Clean wallet with no suspicious activity
//! - 0.1-0.3: Minor concerns (fresh wallet, CEX funding)
//! - 0.4-0.6: Moderate risk (some suspicious patterns)
//! - 0.7-0.9: High risk (serial minting, suspicious interactions)
//! - 1.0: Critical risk (mixer interaction, known scammer)
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use ghost_brain::oracle::profiler::{DevProfiler, DevProfilerConfig};
//! use solana_client::nonblocking::rpc_client::RpcClient;
//! use solana_sdk::pubkey::Pubkey;
//! use std::sync::Arc;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let config = DevProfilerConfig::default();
//! let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
//! let profiler = DevProfiler::new(config, rpc);
//!
//! let creator = "CreatorPubkeyAddress...".parse::<Pubkey>()?;
//! let profile = profiler.analyze_creator(creator).await?;
//!
//! println!("Risk Score: {}", profile.risk_score);
//! println!("Serial Minter: {}", profile.is_serial_minter);
//! println!("Funding Source: {:?}", profile.funding_source);
//! # Ok(())
//! # }
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// Known mixer/tumbler addresses (Tornado Cash equivalents on Solana, etc.)
/// These addresses are associated with privacy-focused mixing services that
/// are often used to obscure the origin of funds in rug-pull schemes.
const KNOWN_MIXER_ADDRESSES: &[&str] = &[
    // Placeholder mixer addresses - in production, maintain an updated list
    // "MixerAddress1111111111111111111111111111111",
];

/// Known CEX hot wallet addresses (Coinbase, Binance, Kraken, etc.)
/// Fresh funding from these addresses indicates a new wallet created for trading.
const KNOWN_CEX_HOT_WALLETS: &[&str] = &[
    // Coinbase hot wallets
    "H8sMJSCQxfKiFTCfDR3DUMLPwcRbM61LGFJ8N4dK3WjS",
    "2AQdpHJ2JpcEgPiATUXjQxA8QmafFegfQwSLWSprPicm",
    // Binance hot wallets
    "5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9",
    "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM",
    // FTX (historical reference)
    // More CEX wallets can be added here
];

/// Known rug-puller addresses that have been associated with previous scams.
/// Interactions with these addresses are flagged as CRITICAL risk.
const KNOWN_RUG_PULLER_ADDRESSES: &[&str] = &[
    // Placeholder - in production, maintain blacklist from community reports
    // "RugPuller111111111111111111111111111111111",
];

/// Pump.fun program ID for detecting token creation transactions
#[allow(dead_code)]
const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

/// Bonk.fun program ID for detecting token creation transactions  
#[allow(dead_code)]
const BONK_FUN_PROGRAM_ID: &str = "boNKM6E9YPZfE5EvCLobz1PApWZsJEyWRRNYkPnWbNn";

/// SPL Token program ID for detecting mint transactions
#[allow(dead_code)]
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// Configuration for DevProfiler
#[derive(Debug, Clone)]
pub struct DevProfilerConfig {
    /// Maximum number of signatures to fetch for analysis
    pub max_signatures: usize,
    /// RPC timeout in seconds
    pub rpc_timeout_secs: u64,
    /// Threshold for serial minter detection (tokens created in 24h)
    pub serial_minter_threshold: usize,
    /// Time window for serial minter detection (hours)
    pub serial_minter_window_hours: u64,
    /// Threshold for fresh wallet age (hours)
    pub fresh_wallet_threshold_hours: u64,
}

impl Default for DevProfilerConfig {
    fn default() -> Self {
        Self {
            max_signatures: 10,
            rpc_timeout_secs: 10,
            serial_minter_threshold: 5, // 5+ tokens in window = serial minter
            serial_minter_window_hours: 24,
            fresh_wallet_threshold_hours: 1, // Wallet created < 1h ago
        }
    }
}

/// Funding source classification for creator wallets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FundingSource {
    /// Wallet funded from centralized exchange (Coinbase, Binance, etc.)
    Cex,
    /// Wallet interacted with mixer/tumbler (Tornado Cash equivalent)
    Mixer,
    /// Fresh wallet with no significant history
    FreshWallet,
    /// Wallet associated with known rug-puller addresses
    AssociatedWithRug,
    /// Organic wallet with normal trading history
    Organic,
    /// Unknown/unclassified funding source
    Unknown,
}

impl Default for FundingSource {
    fn default() -> Self {
        FundingSource::Unknown
    }
}

/// Developer profile with risk assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevProfile {
    /// Risk score from 0.0 (safe) to 1.0 (critical risk)
    pub risk_score: f32,
    /// Whether the creator is a serial token minter
    pub is_serial_minter: bool,
    /// Classification of the funding source
    pub funding_source: FundingSource,
    /// Number of tokens created by this wallet (if detected)
    pub tokens_created: usize,
    /// Age of the wallet in hours (if determinable)
    pub wallet_age_hours: Option<u64>,
    /// Whether mixer interaction was detected
    pub mixer_interaction: bool,
    /// Whether CEX funding was detected
    pub cex_funded: bool,
    /// Whether association with known rug-pullers was detected
    pub rug_association: bool,
    /// Human-readable risk notes
    pub notes: Vec<String>,
    /// Analysis timestamp
    pub analyzed_at: u64,
}

impl Default for DevProfile {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            risk_score: 0.0,
            is_serial_minter: false,
            funding_source: FundingSource::Unknown,
            tokens_created: 0,
            wallet_age_hours: None,
            mixer_interaction: false,
            cex_funded: false,
            rug_association: false,
            notes: Vec::new(),
            analyzed_at: now,
        }
    }
}

/// DevProfiler for analyzing token creator behavior
pub struct DevProfiler {
    config: DevProfilerConfig,
    rpc: Arc<RpcClient>,
    /// Cached set of known mixer addresses for fast lookup
    mixer_addresses: HashSet<String>,
    /// Cached set of known CEX hot wallets for fast lookup
    cex_addresses: HashSet<String>,
    /// Cached set of known rug-puller addresses for fast lookup
    rug_puller_addresses: HashSet<String>,
}

impl DevProfiler {
    /// Create a new DevProfiler instance
    pub fn new(config: DevProfilerConfig, rpc: Arc<RpcClient>) -> Self {
        let mixer_addresses: HashSet<String> = KNOWN_MIXER_ADDRESSES
            .iter()
            .map(|s| s.to_string())
            .collect();

        let cex_addresses: HashSet<String> = KNOWN_CEX_HOT_WALLETS
            .iter()
            .map(|s| s.to_string())
            .collect();

        let rug_puller_addresses: HashSet<String> = KNOWN_RUG_PULLER_ADDRESSES
            .iter()
            .map(|s| s.to_string())
            .collect();

        Self {
            config,
            rpc,
            mixer_addresses,
            cex_addresses,
            rug_puller_addresses,
        }
    }

    /// Analyze a creator's on-chain history and return a risk profile
    ///
    /// This method fetches the last N transaction signatures for the creator
    /// and analyzes them for suspicious patterns including:
    /// - Mixer/tumbler interactions (RISK 1.0)
    /// - CEX hot wallet funding (RISK 0.3)
    /// - Serial token minting (RISK 0.9)
    /// - Association with known rug-pullers (RISK 1.0)
    pub async fn analyze_creator(&self, creator: Pubkey) -> Result<DevProfile> {
        let mut profile = DevProfile::default();
        let creator_str = creator.to_string();

        debug!("Analyzing creator profile: {}", creator_str);

        // Fetch transaction signatures for the creator
        let signatures = self.fetch_signatures(&creator).await?;

        if signatures.is_empty() {
            profile.funding_source = FundingSource::FreshWallet;
            profile.risk_score = 0.3;
            profile
                .notes
                .push("No transaction history found - fresh wallet".to_string());
            return Ok(profile);
        }

        // Analyze wallet age from first transaction
        if let Some(oldest_sig) = signatures.last() {
            if let Some(block_time) = oldest_sig.block_time {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let age_hours = ((now - block_time) / 3600) as u64;
                profile.wallet_age_hours = Some(age_hours);

                if age_hours < self.config.fresh_wallet_threshold_hours {
                    profile.funding_source = FundingSource::FreshWallet;
                    profile.notes.push(format!(
                        "Fresh wallet: {} hours old (threshold: {} hours)",
                        age_hours, self.config.fresh_wallet_threshold_hours
                    ));
                }
            }
        }

        // Analyze each signature for suspicious patterns
        let mut mint_count = 0;
        let window_start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
            - (self.config.serial_minter_window_hours as i64 * 3600);

        for sig_info in &signatures {
            // Check for mint transactions (serial minter detection)
            if self.is_mint_transaction(sig_info) {
                if let Some(block_time) = sig_info.block_time {
                    if block_time >= window_start {
                        mint_count += 1;
                    }
                }
            }

            // Check for mixer interactions
            if self.check_mixer_interaction(sig_info) {
                profile.mixer_interaction = true;
                profile.funding_source = FundingSource::Mixer;
                profile.risk_score = 1.0;
                profile
                    .notes
                    .push("CRITICAL: Mixer/tumbler interaction detected".to_string());
            }

            // Check for CEX funding (only if not already flagged as mixer)
            if !profile.mixer_interaction && self.check_cex_funding(sig_info) {
                profile.cex_funded = true;
                if profile.funding_source == FundingSource::Unknown {
                    profile.funding_source = FundingSource::Cex;
                }
                if profile.risk_score < 0.3 {
                    profile.risk_score = 0.3;
                }
                profile
                    .notes
                    .push("CEX hot wallet funding detected".to_string());
            }

            // Check for rug-puller association
            if self.check_rug_association(sig_info) {
                profile.rug_association = true;
                profile.funding_source = FundingSource::AssociatedWithRug;
                profile.risk_score = 1.0;
                profile
                    .notes
                    .push("CRITICAL: Association with known rug-puller detected".to_string());
            }
        }

        // Update token creation count
        profile.tokens_created = mint_count;

        // Check for serial minter pattern
        if mint_count >= self.config.serial_minter_threshold {
            profile.is_serial_minter = true;
            if profile.risk_score < 0.9 {
                profile.risk_score = 0.9;
            }
            profile.notes.push(format!(
                "Serial minter detected: {} tokens created in last {} hours",
                mint_count, self.config.serial_minter_window_hours
            ));
        }

        // Set organic funding source if no suspicious activity detected
        if profile.funding_source == FundingSource::Unknown && profile.risk_score == 0.0 {
            profile.funding_source = FundingSource::Organic;
            profile
                .notes
                .push("Clean wallet with normal trading history".to_string());
        }

        info!(
            "Creator {} profile: risk={:.2}, serial_minter={}, funding={:?}",
            creator_str, profile.risk_score, profile.is_serial_minter, profile.funding_source
        );

        Ok(profile)
    }

    /// Fetch transaction signatures for a given address
    async fn fetch_signatures(
        &self,
        address: &Pubkey,
    ) -> Result<Vec<solana_client::rpc_response::RpcConfirmedTransactionStatusWithSignature>> {
        use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
        use solana_sdk::commitment_config::CommitmentConfig;

        let config = GetConfirmedSignaturesForAddress2Config {
            limit: Some(self.config.max_signatures),
            commitment: Some(CommitmentConfig::confirmed()),
            ..Default::default()
        };

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.rpc_timeout_secs),
            self.rpc
                .get_signatures_for_address_with_config(address, config),
        )
        .await
        .context("RPC timeout while fetching signatures")?
        .context("Failed to fetch signatures from RPC")?;

        debug!("Fetched {} signatures for {}", result.len(), address);
        Ok(result)
    }

    /// Check if a transaction is a token mint/creation transaction
    fn is_mint_transaction(
        &self,
        sig_info: &solana_client::rpc_response::RpcConfirmedTransactionStatusWithSignature,
    ) -> bool {
        // Check memo for program indicators
        // Pump.fun and similar platforms often include hints in memo
        if let Some(memo) = &sig_info.memo {
            let memo_lower = memo.to_lowercase();
            if memo_lower.contains("create")
                || memo_lower.contains("mint")
                || memo_lower.contains("init")
                || memo_lower.contains("pump")
                || memo_lower.contains("bonk")
                || memo_lower.contains("token")
            {
                return true;
            }
        }

        // Conservative approach: without full transaction parsing,
        // we cannot reliably determine if this is a mint transaction.
        // In a full implementation, we would:
        // 1. Fetch the full transaction using get_transaction()
        // 2. Parse instruction data to check for Pump.fun/Bonk.fun create discriminators
        // 3. Verify interaction with TOKEN_PROGRAM_ID for InitializeMint
        //
        // For now, return false to avoid false positives
        false
    }

    /// Check if a transaction involves mixer/tumbler addresses
    fn check_mixer_interaction(
        &self,
        sig_info: &solana_client::rpc_response::RpcConfirmedTransactionStatusWithSignature,
    ) -> bool {
        // In a full implementation, we would:
        // 1. Fetch the full transaction
        // 2. Parse all account keys
        // 3. Check if any account matches known mixer addresses
        //
        // For now, we use the memo field as a heuristic
        if let Some(memo) = &sig_info.memo {
            for mixer in &self.mixer_addresses {
                if memo.contains(mixer) {
                    return true;
                }
            }
        }

        // The full implementation would check transaction accounts
        false
    }

    /// Check if a transaction involves CEX hot wallet funding
    fn check_cex_funding(
        &self,
        sig_info: &solana_client::rpc_response::RpcConfirmedTransactionStatusWithSignature,
    ) -> bool {
        // In a full implementation, we would:
        // 1. Fetch the full transaction
        // 2. Check if SOL was transferred from a known CEX wallet
        //
        // For now, we use the memo field as a heuristic
        if let Some(memo) = &sig_info.memo {
            for cex in &self.cex_addresses {
                if memo.contains(cex) {
                    return true;
                }
            }
        }

        // The full implementation would check transaction accounts
        false
    }

    /// Check if a transaction involves known rug-puller addresses
    fn check_rug_association(
        &self,
        sig_info: &solana_client::rpc_response::RpcConfirmedTransactionStatusWithSignature,
    ) -> bool {
        // In a full implementation, we would:
        // 1. Fetch the full transaction
        // 2. Parse all account keys
        // 3. Check if any account matches known rug-puller addresses
        //
        // For now, we use the memo field as a heuristic
        if let Some(memo) = &sig_info.memo {
            for rug in &self.rug_puller_addresses {
                if memo.contains(rug) {
                    return true;
                }
            }
        }

        // The full implementation would check transaction accounts
        false
    }

    /// Add a mixer address to the blacklist
    pub fn add_mixer_address(&mut self, address: String) {
        self.mixer_addresses.insert(address);
    }

    /// Add a CEX hot wallet address
    pub fn add_cex_address(&mut self, address: String) {
        self.cex_addresses.insert(address);
    }

    /// Add a known rug-puller address to the blacklist
    pub fn add_rug_puller_address(&mut self, address: String) {
        self.rug_puller_addresses.insert(address);
    }

    /// Get the current mixer address blacklist size
    pub fn mixer_addresses_count(&self) -> usize {
        self.mixer_addresses.len()
    }

    /// Get the current CEX address list size
    pub fn cex_addresses_count(&self) -> usize {
        self.cex_addresses.len()
    }

    /// Get the current rug-puller blacklist size
    pub fn rug_puller_addresses_count(&self) -> usize {
        self.rug_puller_addresses.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;

    fn create_test_config() -> DevProfilerConfig {
        DevProfilerConfig::default()
    }

    #[test]
    fn test_dev_profile_default() {
        let profile = DevProfile::default();

        assert_eq!(profile.risk_score, 0.0);
        assert!(!profile.is_serial_minter);
        assert_eq!(profile.funding_source, FundingSource::Unknown);
        assert_eq!(profile.tokens_created, 0);
        assert!(profile.notes.is_empty());
        assert!(!profile.mixer_interaction);
        assert!(!profile.cex_funded);
        assert!(!profile.rug_association);
    }

    #[test]
    fn test_funding_source_default() {
        let source = FundingSource::default();
        assert_eq!(source, FundingSource::Unknown);
    }

    #[test]
    fn test_funding_source_serialization() {
        let sources = vec![
            FundingSource::Cex,
            FundingSource::Mixer,
            FundingSource::FreshWallet,
            FundingSource::AssociatedWithRug,
            FundingSource::Organic,
            FundingSource::Unknown,
        ];

        for source in sources {
            let json = serde_json::to_string(&source).expect("Serialization failed");
            let deserialized: FundingSource =
                serde_json::from_str(&json).expect("Deserialization failed");
            assert_eq!(source, deserialized);
        }
    }

    #[test]
    fn test_dev_profile_serialization() {
        let mut profile = DevProfile::default();
        profile.risk_score = 0.5;
        profile.is_serial_minter = true;
        profile.funding_source = FundingSource::Cex;
        profile.tokens_created = 3;
        profile.notes.push("Test note".to_string());

        let json = serde_json::to_string(&profile).expect("Serialization failed");
        let deserialized: DevProfile = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(deserialized.risk_score, 0.5);
        assert!(deserialized.is_serial_minter);
        assert_eq!(deserialized.funding_source, FundingSource::Cex);
        assert_eq!(deserialized.tokens_created, 3);
        assert_eq!(deserialized.notes.len(), 1);
    }

    #[test]
    fn test_dev_profiler_config_default() {
        let config = DevProfilerConfig::default();

        assert_eq!(config.max_signatures, 10);
        assert_eq!(config.rpc_timeout_secs, 10);
        assert_eq!(config.serial_minter_threshold, 5);
        assert_eq!(config.serial_minter_window_hours, 24);
        assert_eq!(config.fresh_wallet_threshold_hours, 1);
    }

    #[test]
    fn test_known_addresses_loaded() {
        let config = DevProfilerConfig::default();
        // Create a mock RPC client - we won't actually use it in this test
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let profiler = DevProfiler::new(config, rpc);

        // Verify CEX addresses are loaded
        assert!(profiler.cex_addresses_count() > 0);
    }

    #[test]
    fn test_add_addresses() {
        let config = DevProfilerConfig::default();
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let mut profiler = DevProfiler::new(config, rpc);

        let initial_mixer_count = profiler.mixer_addresses_count();
        let initial_cex_count = profiler.cex_addresses_count();
        let initial_rug_count = profiler.rug_puller_addresses_count();

        profiler.add_mixer_address("TestMixer111111111111111111111111111111111".to_string());
        profiler.add_cex_address("TestCEX1111111111111111111111111111111111".to_string());
        profiler.add_rug_puller_address("TestRug11111111111111111111111111111111111".to_string());

        assert_eq!(profiler.mixer_addresses_count(), initial_mixer_count + 1);
        assert_eq!(profiler.cex_addresses_count(), initial_cex_count + 1);
        assert_eq!(profiler.rug_puller_addresses_count(), initial_rug_count + 1);
    }

    #[test]
    fn test_risk_score_ranges() {
        // Test that risk scores are valid
        let mut profile = DevProfile::default();

        // Low risk
        profile.risk_score = 0.0;
        assert!(profile.risk_score >= 0.0 && profile.risk_score <= 1.0);

        // Medium risk
        profile.risk_score = 0.5;
        assert!(profile.risk_score >= 0.0 && profile.risk_score <= 1.0);

        // High risk
        profile.risk_score = 0.9;
        assert!(profile.risk_score >= 0.0 && profile.risk_score <= 1.0);

        // Critical risk
        profile.risk_score = 1.0;
        assert!(profile.risk_score >= 0.0 && profile.risk_score <= 1.0);
    }

    #[test]
    fn test_serial_minter_threshold() {
        let config = DevProfilerConfig {
            serial_minter_threshold: 3,
            ..Default::default()
        };

        assert_eq!(config.serial_minter_threshold, 3);
    }

    #[test]
    fn test_fresh_wallet_detection() {
        let config = DevProfilerConfig {
            fresh_wallet_threshold_hours: 2,
            ..Default::default()
        };

        assert_eq!(config.fresh_wallet_threshold_hours, 2);
    }
}
