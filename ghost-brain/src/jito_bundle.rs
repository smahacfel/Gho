//! Jito Bundle Batch Execution with Redundancy N+X
//!
//! This module implements high-throughput batch execution of swap intents through
//! Jito bundles with redundancy mechanisms for maximizing inclusion rates.
//!
//! ## Key Features
//! - Pre-allocated SwapIntent pool (≤192 bytes per intent)
//! - Batch processing with leader slot grouping
//! - Multi-tier tip ladder [0.001, 0.005, 0.02, 0.1, 0.5]
//! - N+5 redundancy per SwapIntent in bundle
//! - Fire-and-forget submission with Yellowstone gRPC confirmations
//!
//! ## Performance Targets
//! - ≥98% inclusion rate
//! - 40–120 transactions per bundle
//! - Low-latency submission (<50ms per bundle)

use anyhow::Result;
use object_pool::{Pool, Reusable};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    transaction::VersionedTransaction,
};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

/// Maximum size of SwapIntent: 192 bytes (as per requirement)
pub const SWAP_INTENT_MAX_SIZE: usize = 192;

/// Redundancy level: N+5 (each intent duplicated 6 times total)
pub const REDUNDANCY_LEVEL: u32 = 5;

/// Tip ladder levels (as percentages)
pub const TIP_LADDER: [f64; 5] = [0.001, 0.005, 0.02, 0.1, 0.5];

/// Target transactions per bundle (40-120 range)
pub const MIN_TXS_PER_BUNDLE: usize = 40;
pub const MAX_TXS_PER_BUNDLE: usize = 120;

/// SwapIntent - Compact representation of a swap to be executed
///
/// This structure is designed to be ≤192 bytes and pre-allocated in an object pool.
/// It contains all necessary information for executing a swap on-chain.
#[derive(Debug, Clone)]
pub struct SwapIntent {
    /// Authority that will sign the transaction
    pub authority: Pubkey,

    /// Target AMM pool public key
    pub pool_amm_id: Pubkey,

    /// Amount of input tokens (in lamports)
    pub amount_in: u64,

    /// Minimum acceptable output amount (slippage protection)
    pub min_amount_out: u64,

    /// Unix timestamp when this intent expires
    pub timeout: i64,

    /// Priority level (0.0 = low, 1.0 = high) for tip calculation
    pub priority: f64,

    /// Predicted leader slot for this transaction
    pub predicted_slot: u64,

    /// Token mint being purchased
    pub token_mint: Pubkey,

    /// Internal tracking ID
    pub tracking_id: u64,

    /// Creation timestamp
    pub created_at: i64,

    /// Reserved for future use (padding to ensure size)
    _reserved: [u8; 32],
}

impl SwapIntent {
    /// Create a new SwapIntent
    pub fn new(
        authority: Pubkey,
        pool_amm_id: Pubkey,
        amount_in: u64,
        min_amount_out: u64,
        timeout: i64,
        priority: f64,
        predicted_slot: u64,
        token_mint: Pubkey,
        tracking_id: u64,
    ) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        Self {
            authority,
            pool_amm_id,
            amount_in,
            min_amount_out,
            timeout,
            priority,
            predicted_slot,
            token_mint,
            tracking_id,
            created_at,
            _reserved: [0u8; 32],
        }
    }

    /// Check if this intent has expired
    pub fn is_expired(&self, current_timestamp: i64) -> bool {
        current_timestamp > self.timeout
    }

    /// Calculate the tip amount based on priority and tip ladder
    pub fn calculate_tip(&self, tip_level_index: usize) -> u64 {
        if tip_level_index >= TIP_LADDER.len() {
            return 0;
        }

        let tip_percentage = TIP_LADDER[tip_level_index];
        let base_tip = (self.amount_in as f64 * tip_percentage) as u64;

        // Scale by priority: low priority uses lower tips
        (base_tip as f64 * self.priority.max(0.1).min(1.0)) as u64
    }
}

// Static assertion to ensure SwapIntent is within size limit
const _: () = {
    assert!(std::mem::size_of::<SwapIntent>() <= SWAP_INTENT_MAX_SIZE);
};

/// Pre-allocated pool for SwapIntent objects
static SWAP_INTENT_POOL: Lazy<Pool<SwapIntent>> = Lazy::new(|| {
    Pool::new(1000, || SwapIntent {
        authority: Pubkey::default(),
        pool_amm_id: Pubkey::default(),
        amount_in: 0,
        min_amount_out: 0,
        timeout: 0,
        priority: 0.0,
        predicted_slot: 0,
        token_mint: Pubkey::default(),
        tracking_id: 0,
        created_at: 0,
        _reserved: [0u8; 32],
    })
});

/// Get a SwapIntent from the pre-allocated pool
pub fn get_swap_intent_from_pool() -> Reusable<'static, SwapIntent> {
    SWAP_INTENT_POOL.pull(|| SwapIntent {
        authority: Pubkey::default(),
        pool_amm_id: Pubkey::default(),
        amount_in: 0,
        min_amount_out: 0,
        timeout: 0,
        priority: 0.0,
        predicted_slot: 0,
        token_mint: Pubkey::default(),
        tracking_id: 0,
        created_at: 0,
        _reserved: [0u8; 32],
    })
}

/// Bundle grouped by leader slot
#[derive(Debug)]
pub struct SlotBundle {
    /// Leader slot for this bundle
    pub slot: u64,
    /// Intents to be included in this bundle
    pub intents: Vec<Arc<SwapIntent>>,
}

/// Bundle submission result
#[derive(Debug, Clone)]
pub struct BundleSubmissionResult {
    /// Bundle ID (signature)
    pub bundle_id: Signature,
    /// Number of transactions in bundle
    pub tx_count: usize,
    /// Total tip amount
    pub total_tip: u64,
    /// Submission timestamp
    pub submitted_at: i64,
    /// Tracking IDs of intents in this bundle
    pub intent_ids: Vec<u64>,
}

/// Statistics for batch execution
#[derive(Debug, Default, Clone)]
pub struct BatchExecutionStats {
    /// Total intents processed
    pub total_intents: usize,
    /// Total bundles created
    pub total_bundles: usize,
    /// Total transactions sent
    pub total_transactions: usize,
    /// Total tip paid (in lamports)
    pub total_tip_paid: u64,
    /// Successfully confirmed transactions
    pub confirmed_count: usize,
    /// Failed transactions
    pub failed_count: usize,
    /// Inclusion rate (0.0 - 1.0)
    pub inclusion_rate: f64,
    /// Average transactions per bundle
    pub avg_txs_per_bundle: f64,
}

/// Jito Bundle Executor
///
/// Handles batch execution of swap intents with redundancy and leader slot grouping.
pub struct JitoBundleExecutor {
    /// Jito block engine endpoint
    jito_endpoint: String,
    /// Payer keypair for transaction signing
    payer: Arc<Keypair>,
    /// Recent blockhash cache
    recent_blockhash: RwLock<Option<Hash>>,
    /// Execution statistics
    stats: RwLock<BatchExecutionStats>,
    /// Optional leader predictor for dynamic scheduling
    leader_predictor: Option<Arc<crate::leader_predictor::LeaderPredictor>>,
}

impl JitoBundleExecutor {
    /// Create a new JitoBundleExecutor
    pub fn new(jito_endpoint: String, payer: Arc<Keypair>) -> Self {
        Self {
            jito_endpoint,
            payer,
            recent_blockhash: RwLock::new(None),
            stats: RwLock::new(BatchExecutionStats::default()),
            leader_predictor: None,
        }
    }

    /// Create a new JitoBundleExecutor with leader predictor
    pub fn new_with_leader_predictor(
        jito_endpoint: String,
        payer: Arc<Keypair>,
        leader_predictor: Arc<crate::leader_predictor::LeaderPredictor>,
    ) -> Self {
        Self {
            jito_endpoint,
            payer,
            recent_blockhash: RwLock::new(None),
            stats: RwLock::new(BatchExecutionStats::default()),
            leader_predictor: Some(leader_predictor),
        }
    }

    /// Main entry point: Trigger batch Jito submission with redundancy
    ///
    /// # Arguments
    /// * `batch` - Slice of SwapIntent references to execute
    /// * `redundancy` - Redundancy level (default: 5 for N+5)
    ///
    /// # Returns
    /// * Vector of bundle submission results
    pub async fn trigger_batch_jito(
        &self,
        batch: &[Arc<SwapIntent>],
        redundancy: u32,
    ) -> Result<Vec<BundleSubmissionResult>> {
        if batch.is_empty() {
            return Ok(Vec::new());
        }

        info!(
            "Starting batch Jito execution: {} intents, redundancy=N+{}",
            batch.len(),
            redundancy
        );

        // Step 1: Group intents by predicted leader slot
        let slot_bundles = self.group_by_leader_slot(batch);
        info!("Grouped into {} slot bundles", slot_bundles.len());

        // Step 2: Process each slot bundle
        let mut all_results = Vec::new();

        for slot_bundle in slot_bundles {
            match self.process_slot_bundle(slot_bundle, redundancy).await {
                Ok(mut results) => {
                    all_results.append(&mut results);
                }
                Err(e) => {
                    error!("Failed to process slot bundle: {}", e);
                }
            }
        }

        // Step 3: Update statistics
        self.update_stats(batch.len(), &all_results);

        info!(
            "Batch execution complete: {} bundles submitted, {} total transactions",
            all_results.len(),
            all_results.iter().map(|r| r.tx_count).sum::<usize>()
        );

        Ok(all_results)
    }

    /// Group intents by their predicted leader slot
    fn group_by_leader_slot(&self, batch: &[Arc<SwapIntent>]) -> Vec<SlotBundle> {
        use std::collections::HashMap;

        let mut grouped: HashMap<u64, Vec<Arc<SwapIntent>>> = HashMap::new();

        for intent in batch {
            grouped
                .entry(intent.predicted_slot)
                .or_insert_with(Vec::new)
                .push(Arc::clone(intent));
        }

        grouped
            .into_iter()
            .map(|(slot, intents)| SlotBundle { slot, intents })
            .collect()
    }

    /// Process a single slot bundle with redundancy
    async fn process_slot_bundle(
        &self,
        slot_bundle: SlotBundle,
        redundancy: u32,
    ) -> Result<Vec<BundleSubmissionResult>> {
        let mut results = Vec::new();

        debug!(
            "Processing slot {} with {} intents, redundancy=N+{}",
            slot_bundle.slot,
            slot_bundle.intents.len(),
            redundancy
        );

        // Create N+redundancy bundles (each containing the same intents, but with different tips)
        // This provides redundancy at the bundle level
        for redundancy_idx in 0..=redundancy {
            // Use different tip tiers for different redundancy levels
            let tip_tier_index = (redundancy_idx as usize) % TIP_LADDER.len();

            match self
                .create_and_submit_bundle(
                    &slot_bundle.intents,
                    slot_bundle.slot,
                    tip_tier_index,
                    redundancy_idx,
                )
                .await
            {
                Ok(result) => {
                    results.push(result);
                }
                Err(e) => {
                    warn!(
                        "Failed to submit bundle for slot {} (redundancy {}): {}",
                        slot_bundle.slot, redundancy_idx, e
                    );
                }
            }
        }

        Ok(results)
    }

    /// Create and submit a single bundle
    async fn create_and_submit_bundle(
        &self,
        intents: &[Arc<SwapIntent>],
        slot: u64,
        tip_tier_index: usize,
        redundancy_index: u32,
    ) -> Result<BundleSubmissionResult> {
        // Get recent blockhash
        let blockhash = self.get_or_fetch_blockhash().await?;

        // Get leader-based tip multiplier if leader predictor is available
        // This will apply automatic boost for leaders with land rate <90%
        let leader_tip_multiplier = if let Some(ref predictor) = self.leader_predictor {
            // Derive leader from slot (in production, use actual leader schedule)
            let leader = Self::derive_leader_from_slot(slot);
            let multiplier = predictor.get_tip_multiplier(&leader);
            if multiplier > 1.0 {
                debug!(
                    "Applying {:.1}% tip boost for leader {} at slot {} (historical low performance)",
                    (multiplier - 1.0) * 100.0,
                    leader,
                    slot
                );
            }
            multiplier
        } else {
            1.0
        };

        // Build transactions for each intent
        // With N+5 redundancy, each intent appears (redundancy_index + 1) times in this bundle
        let mut trade_transactions = Vec::new();
        let mut total_tip = 0u64;
        let mut intent_ids = Vec::new();

        // Calculate how many times to duplicate each intent
        // For bundle 0: 1 copy, bundle 1: 2 copies, ..., bundle 5: 6 copies
        let copies_per_intent = (redundancy_index + 1) as usize;

        for intent in intents.iter() {
            // Stop if we would exceed MAX_TXS_PER_BUNDLE
            if trade_transactions.len() + copies_per_intent > MAX_TXS_PER_BUNDLE - 1 {
                break;
            }

            // Calculate base tip and apply leader-based multiplier
            let base_tip = intent.calculate_tip(tip_tier_index);
            let tip = (base_tip as f64 * leader_tip_multiplier) as u64;
            intent_ids.push(intent.tracking_id);

            // Duplicate the transaction `copies_per_intent` times
            for _ in 0..copies_per_intent {
                total_tip += tip;
                // Build the transaction (simplified - actual implementation would use DirectBuyBuilder)
                let tx = self.build_swap_transaction(intent, tip, blockhash)?;
                trade_transactions.push(tx);
            }
        }

        // Ensure bundle order: all trade transactions first, tip transaction last
        let mut transactions = trade_transactions;
        // Add tip payment transaction at the end
        if total_tip > 0 {
            let tip_tx = self.build_tip_transaction(total_tip, blockhash)?;
            transactions.push(tip_tx);
        }

        let tx_count = transactions.len();

        // Submit bundle (fire-and-forget)
        let bundle_id = self
            .submit_bundle_fire_and_forget(transactions, slot)
            .await?;

        debug!(
            "Submitted bundle {} for slot {} (redundancy {}, {} copies/intent, tip tier {}, {} txs, {} lamports tip)",
            bundle_id, slot, redundancy_index, copies_per_intent, tip_tier_index, tx_count, total_tip
        );

        Ok(BundleSubmissionResult {
            bundle_id,
            tx_count,
            total_tip,
            submitted_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            intent_ids,
        })
    }

    /// Build a swap transaction for a given intent
    fn build_swap_transaction(
        &self,
        intent: &SwapIntent,
        _tip: u64,
        blockhash: Hash,
    ) -> Result<VersionedTransaction> {
        // This is a simplified placeholder
        // In production, this would use DirectBuyBuilder to build the actual transaction

        use solana_sdk::{
            instruction::Instruction,
            message::{v0, VersionedMessage},
        };

        // Placeholder instruction (would be replaced with actual DirectBuyBuilder instruction)
        let instruction = Instruction::new_with_bytes(intent.pool_amm_id, &[], vec![]);

        let message =
            v0::Message::try_compile(&self.payer.pubkey(), &[instruction], &[], blockhash)?;

        let versioned_message = VersionedMessage::V0(message);
        let transaction = VersionedTransaction::try_new(versioned_message, &[&*self.payer])?;

        Ok(transaction)
    }

    /// Build a tip payment transaction
    fn build_tip_transaction(
        &self,
        tip_amount: u64,
        blockhash: Hash,
    ) -> Result<VersionedTransaction> {
        use solana_sdk::{
            message::{v0, VersionedMessage},
            system_instruction,
        };

        // Select a random Jito tip account
        let tip_account = Self::select_jito_tip_account();

        let tip_instruction =
            system_instruction::transfer(&self.payer.pubkey(), &tip_account, tip_amount);

        let message =
            v0::Message::try_compile(&self.payer.pubkey(), &[tip_instruction], &[], blockhash)?;

        let versioned_message = VersionedMessage::V0(message);
        let transaction = VersionedTransaction::try_new(versioned_message, &[&*self.payer])?;

        Ok(transaction)
    }

    /// Select a random Jito tip account
    fn select_jito_tip_account() -> Pubkey {
        use rand::Rng;

        const JITO_TIP_ACCOUNTS: &[&str] = &[
            "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
            "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
            "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
            "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
        ];

        let mut rng = rand::thread_rng();
        let index = rng.gen_range(0..JITO_TIP_ACCOUNTS.len());
        JITO_TIP_ACCOUNTS[index].parse().unwrap()
    }

    /// Submit bundle in fire-and-forget mode
    async fn submit_bundle_fire_and_forget(
        &self,
        transactions: Vec<VersionedTransaction>,
        _slot: u64,
    ) -> Result<Signature> {
        // This is a placeholder for actual Jito bundle submission
        // In production, this would use the Jito SDK to submit the bundle

        info!(
            "Submitting bundle with {} transactions to {}",
            transactions.len(),
            self.jito_endpoint
        );

        // For now, return a dummy signature
        // In production: use jito-sdk-rust to submit the bundle
        Ok(Signature::new_unique())
    }

    /// Get or fetch recent blockhash
    async fn get_or_fetch_blockhash(&self) -> Result<Hash> {
        // Check cache first
        {
            let cache = self.recent_blockhash.read();
            if let Some(hash) = *cache {
                return Ok(hash);
            }
        }

        // Fetch new blockhash (placeholder - would use RPC client in production)
        let new_hash = Hash::new_unique();

        {
            let mut cache = self.recent_blockhash.write();
            *cache = Some(new_hash);
        }

        Ok(new_hash)
    }

    /// Update execution statistics
    fn update_stats(&self, intent_count: usize, results: &[BundleSubmissionResult]) {
        let mut stats = self.stats.write();

        stats.total_intents += intent_count;
        stats.total_bundles += results.len();
        stats.total_transactions += results.iter().map(|r| r.tx_count).sum::<usize>();
        stats.total_tip_paid += results.iter().map(|r| r.total_tip).sum::<u64>();

        if stats.total_bundles > 0 {
            stats.avg_txs_per_bundle = stats.total_transactions as f64 / stats.total_bundles as f64;
        }
    }

    /// Get current execution statistics
    pub fn get_stats(&self) -> BatchExecutionStats {
        self.stats.read().clone()
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        let mut stats = self.stats.write();
        *stats = BatchExecutionStats::default();
    }

    /// Derive a leader pubkey from slot number (placeholder implementation)
    ///
    /// In production, this should use the actual leader schedule from the cluster.
    /// For now, we'll use a deterministic derivation for consistency.
    fn derive_leader_from_slot(slot: u64) -> Pubkey {
        // This is a placeholder - in production, we need to fetch the actual
        // leader schedule from the RPC or use the leader predictor's schedule
        let bytes = slot.to_le_bytes();
        let mut key_bytes = [0u8; 32];
        key_bytes[..8].copy_from_slice(&bytes);
        Pubkey::new_from_array(key_bytes)
    }
}

/// Confirmation tracker using Yellowstone gRPC
///
/// Tracks bundle confirmations in the background without blocking submission.
pub struct YellowstoneConfirmationTracker {
    /// Tracking data
    pending_confirmations: RwLock<Vec<BundleSubmissionResult>>,
}

impl YellowstoneConfirmationTracker {
    /// Create a new confirmation tracker
    pub fn new() -> Self {
        Self {
            pending_confirmations: RwLock::new(Vec::new()),
        }
    }

    /// Register a bundle for confirmation tracking
    pub fn track_bundle(&self, result: BundleSubmissionResult) {
        let mut pending = self.pending_confirmations.write();
        pending.push(result);
    }

    /// Start confirmation tracking in background
    ///
    /// This runs in a separate task and updates statistics as confirmations arrive.
    pub async fn start_tracking(&self, _executor: Arc<JitoBundleExecutor>) -> Result<()> {
        // Placeholder for Yellowstone gRPC integration
        // In production, this would:
        // 1. Connect to Yellowstone gRPC
        // 2. Subscribe to transaction confirmations
        // 3. Update executor stats as confirmations arrive

        info!("Starting Yellowstone confirmation tracking (placeholder)");
        Ok(())
    }

    /// Get pending confirmation count
    pub fn pending_count(&self) -> usize {
        self.pending_confirmations.read().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_intent_size() {
        let size = std::mem::size_of::<SwapIntent>();
        println!("SwapIntent size: {} bytes", size);
        assert!(size <= SWAP_INTENT_MAX_SIZE, "SwapIntent exceeds max size");
    }

    #[test]
    fn test_swap_intent_creation() {
        let intent = SwapIntent::new(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            1_000_000,
            900_000,
            1234567890,
            0.5,
            12345,
            Pubkey::new_unique(),
            1,
        );

        assert_eq!(intent.amount_in, 1_000_000);
        assert_eq!(intent.min_amount_out, 900_000);
        assert_eq!(intent.priority, 0.5);
    }

    #[test]
    fn test_tip_calculation() {
        let intent = SwapIntent::new(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            1_000_000_000, // 1 SOL
            900_000_000,
            1234567890,
            1.0, // Max priority
            12345,
            Pubkey::new_unique(),
            1,
        );

        // Tier 0: 0.001 = 1,000,000 lamports
        assert_eq!(intent.calculate_tip(0), 1_000_000);

        // Tier 1: 0.005 = 5,000,000 lamports
        assert_eq!(intent.calculate_tip(1), 5_000_000);

        // Tier 2: 0.02 = 20,000,000 lamports
        assert_eq!(intent.calculate_tip(2), 20_000_000);
    }

    #[test]
    fn test_swap_intent_pool() {
        let intent1 = get_swap_intent_from_pool();
        let intent2 = get_swap_intent_from_pool();

        // Both should be successfully allocated
        assert_eq!(intent1.amount_in, 0); // Default value
        assert_eq!(intent2.amount_in, 0);
    }

    #[tokio::test]
    async fn test_jito_executor_creation() {
        let keypair = Arc::new(Keypair::new());
        let executor =
            JitoBundleExecutor::new("https://mainnet.block-engine.jito.wtf".to_string(), keypair);

        let stats = executor.get_stats();
        assert_eq!(stats.total_intents, 0);
        assert_eq!(stats.total_bundles, 0);
    }

    #[tokio::test]
    async fn test_batch_execution_empty() {
        let keypair = Arc::new(Keypair::new());
        let executor =
            JitoBundleExecutor::new("https://mainnet.block-engine.jito.wtf".to_string(), keypair);

        let batch: Vec<Arc<SwapIntent>> = vec![];
        let result = executor.trigger_batch_jito(&batch, 5).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_batch_execution_with_intents() {
        let keypair = Arc::new(Keypair::new());
        let executor =
            JitoBundleExecutor::new("https://mainnet.block-engine.jito.wtf".to_string(), keypair);

        // Create test intents
        let mut intents = Vec::new();
        for i in 0..10 {
            let intent = Arc::new(SwapIntent::new(
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                1_000_000,
                900_000,
                i64::MAX,
                0.5,
                12345 + (i / 5), // Group into 2 slots
                Pubkey::new_unique(),
                i as u64,
            ));
            intents.push(intent);
        }

        let result = executor.trigger_batch_jito(&intents, 5).await;
        assert!(result.is_ok());

        let bundles = result.unwrap();
        // With redundancy N+5, we should have multiple bundles
        assert!(bundles.len() > 0);

        let stats = executor.get_stats();
        assert_eq!(stats.total_intents, 10);
    }

    #[test]
    fn test_leader_slot_grouping() {
        let keypair = Arc::new(Keypair::new());
        let executor =
            JitoBundleExecutor::new("https://mainnet.block-engine.jito.wtf".to_string(), keypair);

        let intents: Vec<Arc<SwapIntent>> = vec![
            Arc::new(SwapIntent::new(
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                1_000_000,
                900_000,
                i64::MAX,
                0.5,
                100,
                Pubkey::new_unique(),
                1,
            )),
            Arc::new(SwapIntent::new(
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                1_000_000,
                900_000,
                i64::MAX,
                0.5,
                100,
                Pubkey::new_unique(),
                2,
            )),
            Arc::new(SwapIntent::new(
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                1_000_000,
                900_000,
                i64::MAX,
                0.5,
                200,
                Pubkey::new_unique(),
                3,
            )),
        ];

        let bundles = executor.group_by_leader_slot(&intents);

        // Should group into 2 slot bundles (slot 100 and 200)
        assert_eq!(bundles.len(), 2);

        let slot_100_bundle = bundles.iter().find(|b| b.slot == 100);
        assert!(slot_100_bundle.is_some());
        assert_eq!(slot_100_bundle.unwrap().intents.len(), 2);

        let slot_200_bundle = bundles.iter().find(|b| b.slot == 200);
        assert!(slot_200_bundle.is_some());
        assert_eq!(slot_200_bundle.unwrap().intents.len(), 1);
    }
}
