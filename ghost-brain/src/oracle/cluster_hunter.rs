//! ClusterHunter - Cabal Detection System (Wykrywacz Kabalarzy)
//!
//! This module detects "Sniper Clusters" (Cabals) by analyzing funding patterns
//! of top token holders. If multiple holders share the same funding source,
//! this indicates coordinated buying activity - a common rug-pull pattern.
//!
//! # Detection Algorithm
//!
//! 1. Fetch top 20 token accounts for a given mint
//! 2. For each holder, trace the funding source (1-hop depth):
//!    - Find the first SOL transfer into the holder's wallet
//!    - This identifies the "funder" address
//! 3. Build a map: Funder → List<Holder>
//! 4. Detect clusters: If one funder funded >3 holders from Top 20 → Cluster detected
//! 5. Calculate % of supply controlled by the cluster
//! 6. If cluster controls >30% of supply → FLAG HIGH RISK (Panic Sell signal)
//!
//! # Risk Levels
//!
//! - **CRITICAL (1.0)**: Cluster controls >50% supply - almost certain rug
//! - **HIGH (0.8)**: Cluster controls >30% supply - likely coordinated manipulation  
//! - **MODERATE (0.5)**: Cluster controls >20% supply - suspicious concentration
//! - **LOW (0.3)**: Small cluster detected (<20% supply) - monitor closely
//! - **SAFE (0.0)**: No significant clusters detected
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use ghost_brain::oracle::cluster_hunter::{ClusterHunter, ClusterHunterConfig};
//! use solana_client::nonblocking::rpc_client::RpcClient;
//! use solana_sdk::pubkey::Pubkey;
//! use std::sync::Arc;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let config = ClusterHunterConfig::default();
//! let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
//! let hunter = ClusterHunter::new(config, rpc);
//!
//! let mint = "TokenMintAddress...".parse::<Pubkey>()?;
//! let analysis = hunter.analyze_top_holders(mint).await?;
//!
//! if analysis.is_high_risk {
//!     println!("CABAL DETECTED! Cluster controls {}% of supply", analysis.controlled_supply_pct);
//! }
//! # Ok(())
//! # }
//! ```

use anyhow::{Context, Result};
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcProgramAccountsConfig, RpcTransactionConfig};
use solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::UiTransactionEncoding;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;
use tracing::{debug, info, instrument, warn};

/// SPL Token Program ID
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// Known addresses to exclude from cluster analysis (burn addresses, program accounts, etc.)
/// Using &str for lazy static initialization, converted to Pubkey at runtime
const EXCLUDED_ADDRESSES: &[&str] = &[
    "11111111111111111111111111111111",             // System Program
    "1nc1nerator11111111111111111111111111111111",  // Incinerator
    TOKEN_PROGRAM_ID,                               // Token Program
    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL", // Associated Token Program
    "ComputeBudget111111111111111111111111111111",  // Compute Budget Program
    "SysvarRent111111111111111111111111111111111",  // Sysvar Rent
];

/// Helper function to check if a Pubkey is in the excluded list
fn is_excluded_address(pubkey: &Pubkey) -> bool {
    let pubkey_str = pubkey.to_string();
    EXCLUDED_ADDRESSES.contains(&pubkey_str.as_str())
}

/// Token account data layout constants
/// Token account structure: mint(32) + owner(32) + amount(8) + ...
const TOKEN_ACCOUNT_MIN_SIZE: usize = 72;
const TOKEN_ACCOUNT_OWNER_OFFSET: usize = 32;
const TOKEN_ACCOUNT_AMOUNT_OFFSET: usize = 64;
const TOKEN_ACCOUNT_AMOUNT_SIZE: usize = 8;

/// Display truncation length for funder addresses in notes
const FUNDER_DISPLAY_LENGTH: usize = 8;

/// Cluster metrics summarizing the detected cluster
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ClusterMetric {
    /// Maximum number of holders funded by a single address
    pub max_cluster_size: usize,
    /// Percentage of total supply controlled by the largest cluster (0-100)
    pub controlled_supply_pct: f32,
    /// Address of the primary funder (the one funding the most holders)
    pub primary_funder: Option<String>,
    /// Number of distinct clusters detected (funder with >1 holder)
    pub cluster_count: usize,
    /// Total number of holders in all clusters combined
    pub total_clustered_holders: usize,
}

/// Individual holder information with funding trace
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HolderFunding {
    /// Holder's wallet address
    pub holder_address: String,
    /// Token account address
    pub token_account: String,
    /// Token balance held
    pub balance: u64,
    /// Percentage of total supply (0-100)
    pub supply_percentage: f32,
    /// Funder address (who sent the first SOL to this wallet)
    pub funder_address: Option<String>,
    /// Whether this holder is part of a cluster
    pub is_clustered: bool,
}

/// Complete cluster analysis result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClusterAnalysis {
    /// Token mint address being analyzed
    pub mint: String,
    /// Total token supply
    pub total_supply: u64,
    /// Cluster detection metrics
    pub metrics: ClusterMetric,
    /// List of top holders with funding information
    pub holders: Vec<HolderFunding>,
    /// Map of funder address → list of holder addresses funded by them
    pub funder_map: HashMap<String, Vec<String>>,
    /// Whether this token is flagged as high risk (cluster controls >30% supply)
    pub is_high_risk: bool,
    /// Risk score from 0.0 (safe) to 1.0 (critical)
    pub risk_score: f32,
    /// Human-readable risk notes
    pub notes: Vec<String>,
    /// Analysis timestamp (Unix seconds)
    pub analyzed_at: u64,
    /// Analysis duration in milliseconds
    pub analysis_time_ms: u64,
}

impl Default for ClusterAnalysis {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            mint: String::new(),
            total_supply: 0,
            metrics: ClusterMetric::default(),
            holders: Vec::new(),
            funder_map: HashMap::new(),
            is_high_risk: false,
            risk_score: 0.0,
            notes: Vec::new(),
            analyzed_at: now,
            analysis_time_ms: 0,
        }
    }
}

/// Configuration for ClusterHunter
#[derive(Debug, Clone)]
pub struct ClusterHunterConfig {
    /// Number of top holders to analyze (default: 20)
    pub top_holders_count: usize,
    /// Minimum cluster size to flag (default: 3 = same funder funded 3+ holders)
    pub min_cluster_size: usize,
    /// Supply percentage threshold for high risk flag (default: 30%)
    pub high_risk_threshold_pct: f32,
    /// RPC timeout in seconds for each RPC call
    pub rpc_timeout_secs: u64,
    /// Maximum signatures to fetch per holder for funding trace
    pub max_signatures_per_holder: usize,
    /// Enable caching of funding source lookups
    pub enable_cache: bool,
    /// Cache TTL in seconds
    pub cache_ttl_secs: u64,
}

impl Default for ClusterHunterConfig {
    fn default() -> Self {
        Self {
            top_holders_count: 20,
            min_cluster_size: 3,
            high_risk_threshold_pct: 30.0,
            rpc_timeout_secs: 10,
            max_signatures_per_holder: 20,
            enable_cache: true,
            cache_ttl_secs: 300, // 5 minutes
        }
    }
}

/// ClusterHunter - Detects Cabal/Sniper Clusters in token holder distributions
pub struct ClusterHunter {
    config: ClusterHunterConfig,
    rpc: Arc<RpcClient>,
    /// Cache for funding source lookups: holder_pubkey -> (funder_pubkey, timestamp)
    /// Using Pubkey instead of String for zero-copy performance (32 bytes on stack vs heap allocation)
    funding_cache: Option<Arc<tokio::sync::RwLock<HashMap<Pubkey, (Option<Pubkey>, Instant)>>>>,
}

impl ClusterHunter {
    /// Create a new ClusterHunter instance
    pub fn new(config: ClusterHunterConfig, rpc: Arc<RpcClient>) -> Self {
        info!(
            "Initialized ClusterHunter: top_holders={}, min_cluster={}, high_risk_threshold={}%",
            config.top_holders_count, config.min_cluster_size, config.high_risk_threshold_pct
        );

        let funding_cache = if config.enable_cache {
            Some(Arc::new(tokio::sync::RwLock::new(HashMap::new())))
        } else {
            None
        };

        Self {
            config,
            rpc,
            funding_cache,
        }
    }

    /// Analyze top holders of a token to detect funding clusters (Cabals)
    ///
    /// # Arguments
    /// * `token_mint` - The token mint address to analyze
    ///
    /// # Returns
    /// Complete cluster analysis with risk assessment
    #[instrument(skip(self))]
    pub async fn analyze_top_holders(&self, token_mint: Pubkey) -> Result<ClusterAnalysis> {
        let start_time = Instant::now();
        let mint_str = token_mint.to_string();

        info!("Starting cluster analysis for mint={}", mint_str);

        let mut analysis = ClusterAnalysis {
            mint: mint_str.clone(),
            ..Default::default()
        };

        // Step 1: Get token supply
        let supply_result = timeout(
            Duration::from_secs(self.config.rpc_timeout_secs),
            self.rpc.get_token_supply(&token_mint),
        )
        .await
        .context("Timeout getting token supply")?
        .context("Failed to get token supply")?;

        let total_supply = supply_result
            .amount
            .parse::<u64>()
            .context("Failed to parse token supply")?;

        analysis.total_supply = total_supply;

        if total_supply == 0 {
            analysis.notes.push("Token has zero supply".to_string());
            analysis.analysis_time_ms = start_time.elapsed().as_millis() as u64;
            return Ok(analysis);
        }

        // Step 2: Fetch top holders
        let holders = self
            .fetch_top_holders(&token_mint, total_supply)
            .await
            .context("Failed to fetch top holders")?;

        if holders.is_empty() {
            analysis.notes.push("No holders found".to_string());
            analysis.analysis_time_ms = start_time.elapsed().as_millis() as u64;
            return Ok(analysis);
        }

        debug!("Found {} top holders for {}", holders.len(), mint_str);

        // Step 3: Trace funding source for each holder (1-hop depth) - PARALLEL RPC calls
        // This reduces time from O(n * latency) to O(latency) by running all requests concurrently
        // Using Pubkey internally for zero-copy performance
        let funding_futures = holders.iter().map(|(token_account, owner, balance)| {
            let owner_copy = *owner;
            let token_account_copy = *token_account;
            let balance_copy = *balance;
            async move {
                let funder = self
                    .trace_funding_source(&owner_copy)
                    .await
                    .unwrap_or_else(|e| {
                        debug!("Failed to trace funder for {}: {}", owner_copy, e);
                        None
                    });
                (token_account_copy, owner_copy, balance_copy, funder)
            }
        });

        // Fire all RPC requests simultaneously and wait for all results
        let funding_results = join_all(funding_futures).await;

        // Process results using Pubkey internally (in-memory, ultra fast, zero-copy)
        // Internal map uses Pubkey for O(1) lookups without string conversion overhead
        let mut internal_funder_map: HashMap<Pubkey, Vec<Pubkey>> = HashMap::new();
        let mut holder_data: Vec<(Pubkey, Pubkey, u64, f32, Option<Pubkey>)> = Vec::new();

        for (token_account, owner, balance, funder) in funding_results {
            let supply_pct = (balance as f32 / total_supply as f32) * 100.0;
            holder_data.push((token_account, owner, balance, supply_pct, funder));

            // Add to funder map if we found a funder
            if let Some(funder_pubkey) = funder {
                internal_funder_map
                    .entry(funder_pubkey)
                    .or_default()
                    .push(owner);
            }
        }

        // Step 4: Analyze clusters using Pubkey (fast comparisons)
        let mut max_cluster_size = 0;
        let mut primary_funder_pubkey: Option<Pubkey> = None;
        let mut cluster_count = 0;
        let mut total_clustered_holders = 0;
        let mut cluster_supply_pct: f32 = 0.0;
        let mut clustered_owners: std::collections::HashSet<Pubkey> =
            std::collections::HashSet::new();

        for (funder, funded_holders) in &internal_funder_map {
            if funded_holders.len() >= self.config.min_cluster_size {
                cluster_count += 1;
                total_clustered_holders += funded_holders.len();

                // Calculate supply controlled by this cluster
                let cluster_supply: f32 = holder_data
                    .iter()
                    .filter(|(_, owner, _, _, _)| funded_holders.contains(owner))
                    .map(|(_, _, _, supply_pct, _)| supply_pct)
                    .sum();

                if funded_holders.len() > max_cluster_size {
                    max_cluster_size = funded_holders.len();
                    primary_funder_pubkey = Some(*funder);
                    cluster_supply_pct = cluster_supply;
                }

                // Mark holders as clustered
                for owner in funded_holders {
                    clustered_owners.insert(*owner);
                }

                // Add note with truncated funder address
                let funder_str = funder.to_string();
                analysis.notes.push(format!(
                    "Cluster detected: Funder {} funded {} holders ({:.1}% supply)",
                    &funder_str[..FUNDER_DISPLAY_LENGTH.min(funder_str.len())],
                    funded_holders.len(),
                    cluster_supply
                ));
            }
        }

        // Step 5: Calculate risk score
        let (risk_score, is_high_risk) =
            self.calculate_risk_score(max_cluster_size, cluster_supply_pct, cluster_count);

        // Step 6: Convert to String only at the very end for JSON output
        // This is the ONLY place where we convert Pubkey -> String
        let holder_fundings: Vec<HolderFunding> = holder_data
            .iter()
            .map(
                |(token_account, owner, balance, supply_pct, funder)| HolderFunding {
                    holder_address: owner.to_string(),
                    token_account: token_account.to_string(),
                    balance: *balance,
                    supply_percentage: *supply_pct,
                    funder_address: funder.map(|f| f.to_string()),
                    is_clustered: clustered_owners.contains(owner),
                },
            )
            .collect();

        let funder_map: HashMap<String, Vec<String>> = internal_funder_map
            .iter()
            .map(|(funder, holders)| {
                (
                    funder.to_string(),
                    holders.iter().map(|h| h.to_string()).collect(),
                )
            })
            .collect();

        // Populate metrics
        analysis.metrics = ClusterMetric {
            max_cluster_size,
            controlled_supply_pct: cluster_supply_pct,
            primary_funder: primary_funder_pubkey.map(|p| p.to_string()),
            cluster_count,
            total_clustered_holders,
        };

        analysis.holders = holder_fundings;
        analysis.funder_map = funder_map;
        analysis.is_high_risk = is_high_risk;
        analysis.risk_score = risk_score;

        if is_high_risk {
            analysis.notes.push(format!(
                "HIGH RISK: Cluster controls {:.1}% of supply (threshold: {}%)",
                cluster_supply_pct, self.config.high_risk_threshold_pct
            ));
        }

        analysis.analyzed_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        analysis.analysis_time_ms = start_time.elapsed().as_millis() as u64;

        info!(
            "Cluster analysis complete for {}: clusters={}, max_size={}, supply_pct={:.1}%, risk={:.2}, high_risk={}, time={}ms",
            mint_str,
            cluster_count,
            max_cluster_size,
            cluster_supply_pct,
            risk_score,
            is_high_risk,
            analysis.analysis_time_ms
        );

        Ok(analysis)
    }

    /// Fetch top N holders of a token
    /// Returns Vec<(token_account_pubkey, owner_pubkey, balance)> - using Pubkey for zero-copy performance
    async fn fetch_top_holders(
        &self,
        mint: &Pubkey,
        _total_supply: u64,
    ) -> Result<Vec<(Pubkey, Pubkey, u64)>> {
        debug!(
            "Fetching top {} holders for {}",
            self.config.top_holders_count, mint
        );

        // Create filter to get all token accounts for this mint
        let mint_bytes = mint.to_bytes();
        let config = RpcProgramAccountsConfig {
            filters: Some(vec![
                RpcFilterType::Memcmp(Memcmp::new(
                    0, // Offset 0 is the mint address in token account
                    MemcmpEncodedBytes::Bytes(mint_bytes.to_vec()),
                )),
                RpcFilterType::DataSize(165), // Token account size
            ]),
            account_config: Default::default(),
            ..Default::default()
        };

        let token_program =
            Pubkey::from_str(TOKEN_PROGRAM_ID).expect("Invalid token program ID constant");

        let accounts = timeout(
            Duration::from_secs(self.config.rpc_timeout_secs),
            self.rpc
                .get_program_accounts_with_config(&token_program, config),
        )
        .await
        .context("Timeout fetching token accounts")?
        .context("Failed to fetch token accounts")?;

        debug!("Found {} token accounts for {}", accounts.len(), mint);

        // Parse and extract holder info - using Pubkey directly (no String conversion)
        let mut holders: Vec<(Pubkey, Pubkey, u64)> = Vec::new();

        for (token_account_pubkey, account) in &accounts {
            if account.data.len() >= TOKEN_ACCOUNT_MIN_SIZE {
                // Extract owner (32 bytes at offset 32)
                let owner_bytes: [u8; 32] = account.data
                    [TOKEN_ACCOUNT_OWNER_OFFSET..TOKEN_ACCOUNT_OWNER_OFFSET + 32]
                    .try_into()
                    .context("Failed to extract owner bytes")?;
                let owner = Pubkey::new_from_array(owner_bytes);

                // Skip excluded addresses (using helper function)
                if is_excluded_address(&owner) {
                    continue;
                }

                // Extract amount (8 bytes at offset 64)
                let amount_bytes: [u8; TOKEN_ACCOUNT_AMOUNT_SIZE] = account.data
                    [TOKEN_ACCOUNT_AMOUNT_OFFSET
                        ..TOKEN_ACCOUNT_AMOUNT_OFFSET + TOKEN_ACCOUNT_AMOUNT_SIZE]
                    .try_into()
                    .context("Failed to extract amount bytes")?;
                let amount = u64::from_le_bytes(amount_bytes);

                // Only include accounts with non-zero balance
                if amount > 0 {
                    holders.push((*token_account_pubkey, owner, amount));
                }
            }
        }

        // Sort by balance descending and take top N
        holders.sort_by(|a, b| b.2.cmp(&a.2));
        holders.truncate(self.config.top_holders_count);

        debug!(
            "Returning {} top holders (max: {})",
            holders.len(),
            self.config.top_holders_count
        );

        Ok(holders)
    }

    /// Trace the funding source (funder) of a holder wallet
    ///
    /// This performs a 1-hop trace to find who first sent SOL to this wallet.
    /// The funder is typically the address that initiated the wallet.
    /// Uses Pubkey internally for zero-copy performance.
    async fn trace_funding_source(&self, holder: &Pubkey) -> Result<Option<Pubkey>> {
        // Check cache first
        if let Some(cache) = &self.funding_cache {
            let cache_read = cache.read().await;
            if let Some((funder, cached_at)) = cache_read.get(holder) {
                if cached_at.elapsed().as_secs() < self.config.cache_ttl_secs {
                    debug!("Cache hit for funder of {}", holder);
                    return Ok(*funder);
                }
            }
        }

        // Fetch recent signatures for the holder
        let signatures = timeout(
            Duration::from_secs(self.config.rpc_timeout_secs),
            self.rpc.get_signatures_for_address(holder),
        )
        .await
        .context("Timeout fetching signatures")?
        .context("Failed to fetch signatures")?;

        if signatures.is_empty() {
            debug!("No signatures found for holder {}", holder);
            self.cache_funder(holder, None).await;
            return Ok(None);
        }

        // Get the oldest signature (first transaction - likely the funding tx)
        // Take up to max_signatures and look at the oldest
        let oldest_sigs: Vec<_> = signatures
            .iter()
            .take(self.config.max_signatures_per_holder)
            .collect();

        // Start from the oldest transaction
        for sig_info in oldest_sigs.iter().rev() {
            let signature =
                Signature::from_str(&sig_info.signature).context("Invalid signature")?;

            // Fetch full transaction to analyze
            let tx_config = RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::JsonParsed),
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(0),
            };

            let tx_result = timeout(
                Duration::from_secs(self.config.rpc_timeout_secs),
                self.rpc.get_transaction_with_config(&signature, tx_config),
            )
            .await;

            match tx_result {
                Ok(Ok(tx)) => {
                    // Try to extract the funder from this transaction
                    if let Some(funder) = self.extract_funder_from_transaction(&tx, holder) {
                        debug!("Found funder {} for holder {}", funder, holder);
                        self.cache_funder(holder, Some(funder)).await;
                        return Ok(Some(funder));
                    }
                }
                Ok(Err(e)) => {
                    debug!("Failed to fetch transaction {}: {}", sig_info.signature, e);
                }
                Err(_) => {
                    debug!("Timeout fetching transaction {}", sig_info.signature);
                }
            }
        }

        debug!("Could not determine funder for holder {}", holder);
        self.cache_funder(holder, None).await;
        Ok(None)
    }

    /// Extract the funder address from a transaction
    ///
    /// The funder is the account that sent SOL to the holder in this transaction.
    /// Returns Pubkey directly for zero-copy performance.
    fn extract_funder_from_transaction(
        &self,
        tx: &solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta,
        holder: &Pubkey,
    ) -> Option<Pubkey> {
        // Extract account keys from transaction
        let transaction = tx.transaction.transaction.decode()?;
        let message = &transaction.message;
        let account_keys = message.static_account_keys();

        if account_keys.is_empty() {
            return None;
        }

        // The fee payer (first account) is typically the initiator
        let fee_payer = account_keys.first()?;

        // If the fee payer is not the holder, it's likely the funder
        if fee_payer != holder && !is_excluded_address(fee_payer) {
            return Some(*fee_payer);
        }

        // Look for other signers that might be the funder
        for key in account_keys.iter().skip(1) {
            if key != holder && !is_excluded_address(key) {
                // Check if this might be a transfer source
                // For simplicity, we take the second account as a potential funder
                return Some(*key);
            }
        }

        None
    }

    /// Cache a funder lookup result
    async fn cache_funder(&self, holder: &Pubkey, funder: Option<Pubkey>) {
        if let Some(cache) = &self.funding_cache {
            let mut cache_write = cache.write().await;
            cache_write.insert(*holder, (funder, Instant::now()));
        }
    }

    /// Calculate risk score based on cluster metrics
    ///
    /// Returns (risk_score, is_high_risk)
    fn calculate_risk_score(
        &self,
        max_cluster_size: usize,
        controlled_supply_pct: f32,
        cluster_count: usize,
    ) -> (f32, bool) {
        let mut risk_score: f32 = 0.0;

        // Factor 1: Cluster size contribution (max 0.3)
        // Larger clusters are more concerning
        let size_factor = if max_cluster_size >= 10 {
            0.3
        } else if max_cluster_size >= 5 {
            0.2
        } else if max_cluster_size >= self.config.min_cluster_size {
            0.1
        } else {
            0.0
        };
        risk_score += size_factor;

        // Factor 2: Supply concentration contribution (max 0.6)
        // More supply in cluster = higher risk
        let supply_factor = if controlled_supply_pct >= 50.0 {
            0.6 // Critical: >50% supply in cluster
        } else if controlled_supply_pct >= 30.0 {
            0.4 // High: >30% supply
        } else if controlled_supply_pct >= 20.0 {
            0.2 // Moderate: >20% supply
        } else if controlled_supply_pct >= 10.0 {
            0.1 // Low: >10% supply
        } else {
            0.0
        };
        risk_score += supply_factor;

        // Factor 3: Multiple clusters contribution (max 0.1)
        // Multiple distinct clusters suggest sophisticated manipulation
        if cluster_count >= 3 {
            risk_score += 0.1;
        } else if cluster_count >= 2 {
            risk_score += 0.05;
        }

        // Clamp to [0, 1]
        risk_score = risk_score.min(1.0);

        // High risk threshold
        let is_high_risk = controlled_supply_pct >= self.config.high_risk_threshold_pct;

        (risk_score, is_high_risk)
    }

    /// Clear the funding cache
    pub async fn clear_cache(&self) {
        if let Some(cache) = &self.funding_cache {
            let mut cache_write = cache.write().await;
            cache_write.clear();
            info!("Funding cache cleared");
        }
    }

    /// Get current cache size
    pub async fn cache_size(&self) -> usize {
        if let Some(cache) = &self.funding_cache {
            cache.read().await.len()
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> ClusterHunterConfig {
        ClusterHunterConfig::default()
    }

    #[test]
    fn test_cluster_metric_default() {
        let metric = ClusterMetric::default();

        assert_eq!(metric.max_cluster_size, 0);
        assert_eq!(metric.controlled_supply_pct, 0.0);
        assert!(metric.primary_funder.is_none());
        assert_eq!(metric.cluster_count, 0);
        assert_eq!(metric.total_clustered_holders, 0);
    }

    #[test]
    fn test_cluster_analysis_default() {
        let analysis = ClusterAnalysis::default();

        assert!(analysis.mint.is_empty());
        assert_eq!(analysis.total_supply, 0);
        assert!(!analysis.is_high_risk);
        assert_eq!(analysis.risk_score, 0.0);
        assert!(analysis.holders.is_empty());
        assert!(analysis.funder_map.is_empty());
        assert!(analysis.notes.is_empty());
    }

    #[test]
    fn test_cluster_hunter_config_default() {
        let config = ClusterHunterConfig::default();

        assert_eq!(config.top_holders_count, 20);
        assert_eq!(config.min_cluster_size, 3);
        assert_eq!(config.high_risk_threshold_pct, 30.0);
        assert_eq!(config.rpc_timeout_secs, 10);
        assert_eq!(config.max_signatures_per_holder, 20);
        assert!(config.enable_cache);
        assert_eq!(config.cache_ttl_secs, 300);
    }

    #[test]
    fn test_holder_funding_serialization() {
        let funding = HolderFunding {
            holder_address: "holder123".to_string(),
            token_account: "tokenAcc123".to_string(),
            balance: 1000000,
            supply_percentage: 5.5,
            funder_address: Some("funder456".to_string()),
            is_clustered: true,
        };

        let json = serde_json::to_string(&funding).expect("Serialization failed");
        let deserialized: HolderFunding =
            serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(deserialized.holder_address, "holder123");
        assert_eq!(deserialized.balance, 1000000);
        assert_eq!(deserialized.supply_percentage, 5.5);
        assert_eq!(deserialized.funder_address, Some("funder456".to_string()));
        assert!(deserialized.is_clustered);
    }

    #[test]
    fn test_cluster_analysis_serialization() {
        let mut analysis = ClusterAnalysis::default();
        analysis.mint = "testMint123".to_string();
        analysis.total_supply = 1000000000;
        analysis.is_high_risk = true;
        analysis.risk_score = 0.85;
        analysis.notes.push("Test cluster detected".to_string());
        analysis.funder_map.insert(
            "funder1".to_string(),
            vec!["holder1".to_string(), "holder2".to_string()],
        );

        let json = serde_json::to_string(&analysis).expect("Serialization failed");
        let deserialized: ClusterAnalysis =
            serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(deserialized.mint, "testMint123");
        assert_eq!(deserialized.total_supply, 1000000000);
        assert!(deserialized.is_high_risk);
        assert_eq!(deserialized.risk_score, 0.85);
        assert_eq!(deserialized.notes.len(), 1);
        assert!(deserialized.funder_map.contains_key("funder1"));
    }

    #[test]
    fn test_risk_score_calculation_no_cluster() {
        let config = ClusterHunterConfig::default();
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let hunter = ClusterHunter::new(config, rpc);

        let (risk_score, is_high_risk) = hunter.calculate_risk_score(0, 0.0, 0);

        assert_eq!(risk_score, 0.0);
        assert!(!is_high_risk);
    }

    #[test]
    fn test_risk_score_calculation_small_cluster() {
        let config = ClusterHunterConfig::default();
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let hunter = ClusterHunter::new(config, rpc);

        // Cluster of 3 holders controlling 15% supply
        let (risk_score, is_high_risk) = hunter.calculate_risk_score(3, 15.0, 1);

        assert!(risk_score > 0.0 && risk_score < 0.5);
        assert!(!is_high_risk); // Below 30% threshold
    }

    #[test]
    fn test_risk_score_calculation_medium_cluster() {
        let config = ClusterHunterConfig::default();
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let hunter = ClusterHunter::new(config, rpc);

        // Cluster of 5 holders controlling 25% supply
        let (risk_score, is_high_risk) = hunter.calculate_risk_score(5, 25.0, 1);

        assert!(risk_score > 0.3 && risk_score < 0.7);
        assert!(!is_high_risk); // Below 30% threshold
    }

    #[test]
    fn test_risk_score_calculation_high_risk_cluster() {
        let config = ClusterHunterConfig::default();
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let hunter = ClusterHunter::new(config, rpc);

        // Cluster of 8 holders controlling 35% supply
        let (risk_score, is_high_risk) = hunter.calculate_risk_score(8, 35.0, 1);

        assert!(risk_score >= 0.5);
        assert!(is_high_risk); // Above 30% threshold
    }

    #[test]
    fn test_risk_score_calculation_critical_cluster() {
        let config = ClusterHunterConfig::default();
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let hunter = ClusterHunter::new(config, rpc);

        // Large cluster of 12 holders controlling 55% supply with 3 distinct clusters
        let (risk_score, is_high_risk) = hunter.calculate_risk_score(12, 55.0, 3);

        assert!(risk_score >= 0.9);
        assert!(is_high_risk);
    }

    #[test]
    fn test_risk_score_clamped_to_one() {
        let config = ClusterHunterConfig::default();
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let hunter = ClusterHunter::new(config, rpc);

        // Extreme case: should not exceed 1.0
        let (risk_score, _) = hunter.calculate_risk_score(100, 100.0, 10);

        assert!(risk_score <= 1.0);
    }

    #[test]
    fn test_excluded_addresses() {
        // Verify that system program and other excluded addresses are in the list
        assert!(EXCLUDED_ADDRESSES.contains(&"11111111111111111111111111111111"));
        assert!(EXCLUDED_ADDRESSES.contains(&"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"));
    }

    #[test]
    fn test_custom_config() {
        let config = ClusterHunterConfig {
            top_holders_count: 50,
            min_cluster_size: 5,
            high_risk_threshold_pct: 40.0,
            rpc_timeout_secs: 15,
            max_signatures_per_holder: 30,
            enable_cache: false,
            cache_ttl_secs: 600,
        };

        assert_eq!(config.top_holders_count, 50);
        assert_eq!(config.min_cluster_size, 5);
        assert_eq!(config.high_risk_threshold_pct, 40.0);
        assert_eq!(config.rpc_timeout_secs, 15);
        assert!(!config.enable_cache);
    }

    #[test]
    fn test_funder_map_aggregation() {
        // Simulate funder map aggregation logic
        let mut funder_map: HashMap<String, Vec<String>> = HashMap::new();

        // Funder A funded 4 holders
        funder_map
            .entry("FunderA".to_string())
            .or_default()
            .push("Holder1".to_string());
        funder_map
            .entry("FunderA".to_string())
            .or_default()
            .push("Holder2".to_string());
        funder_map
            .entry("FunderA".to_string())
            .or_default()
            .push("Holder3".to_string());
        funder_map
            .entry("FunderA".to_string())
            .or_default()
            .push("Holder4".to_string());

        // Funder B funded 2 holders
        funder_map
            .entry("FunderB".to_string())
            .or_default()
            .push("Holder5".to_string());
        funder_map
            .entry("FunderB".to_string())
            .or_default()
            .push("Holder6".to_string());

        // Find max cluster
        let max_cluster = funder_map.values().map(|v| v.len()).max().unwrap_or(0);
        assert_eq!(max_cluster, 4);

        // Count clusters with size >= 3
        let cluster_count = funder_map.values().filter(|v| v.len() >= 3).count();
        assert_eq!(cluster_count, 1);
    }
}
