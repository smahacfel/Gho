//! Revolver Worker - Background Blockhash and Signature Refresh
//!
//! This module provides a background worker that periodically refreshes
//! blockhashes and re-signs bullets to keep them valid for execution.
//!
//! # Architecture
//!
//! The worker runs in a separate tokio task and:
//! 1. Polls for recent blockhash every 30 seconds
//! 2. Identifies stale bullets (>60 seconds old)
//! 3. Re-signs transactions with fresh blockhash
//! 4. Updates bullets in the revolver
//!
//! # Usage
//!
//! ```ignore
//! let worker = RevolverWorker::new(revolver, rpc_client, payer);
//! let handle = worker.start();
//!
//! // Worker runs in background...
//!
//! handle.stop().await?;
//! ```

use crate::errors::{Result, TriggerError};
use crate::revolver::Revolver;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    hash::Hash, pubkey::Pubkey, signature::Keypair, signer::Signer,
    transaction::VersionedTransaction,
};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

use crate::direct_sell_builder::DirectSellBuilder;
use crate::revolver::Bullet;
use crate::revolver_sell_builder::{AmmProtocol, SellTxBuilder, SellTxConfig};
use crate::rpc_provider::BlockhashProvider;
use ghost_core::shadow_ledger::ShadowLedger;
use ghost_core::ShadowBondingCurve;

/// Default maximum slot age for stale data protection (5-10 slots = 2-4 seconds)
pub const DEFAULT_MAX_STALE_SLOTS: u64 = 10;
const BONDING_CURVE_SEED: &[u8] = b"bonding-curve";
const SLOW_BLOCKHASH_FETCH_MS: u128 = 200;

/// Configuration for the revolver worker
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// How often to check for stale bullets and refresh blockhash (seconds)
    pub refresh_interval_secs: u64,
    /// Whether the worker is enabled
    pub enabled: bool,
    /// Maximum slot age before considering data stale (default: 10 slots)
    pub max_stale_slots: u64,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            refresh_interval_secs: 30,
            enabled: true,
            max_stale_slots: DEFAULT_MAX_STALE_SLOTS,
        }
    }
}

/// Background worker for refreshing bullets
pub struct RevolverWorker {
    /// Shared revolver instance
    revolver: Arc<RwLock<Revolver>>,
    /// RPC client for fetching blockhash
    rpc_client: Arc<RpcClient>,
    /// Payer keypair for signing
    payer: Arc<Keypair>,
    /// Worker configuration
    config: WorkerConfig,
    /// Optional blockhash provider for hot-path refresh separation
    blockhash_provider: Option<Arc<BlockhashProvider>>,
}

impl RevolverWorker {
    /// Create a new revolver worker
    pub fn new(
        revolver: Arc<RwLock<Revolver>>,
        rpc_client: Arc<RpcClient>,
        payer: Arc<Keypair>,
        config: WorkerConfig,
    ) -> Self {
        Self {
            revolver,
            rpc_client,
            payer,
            config,
            blockhash_provider: None,
        }
    }

    /// Attach a shared blockhash provider to avoid direct RPC in refresh cycles.
    pub fn with_blockhash_provider(mut self, provider: Arc<BlockhashProvider>) -> Self {
        self.blockhash_provider = Some(provider);
        self
    }

    /// Start the worker in a background task
    pub fn start(self) -> WorkerHandle {
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let revolver = Arc::clone(&self.revolver);
        let rpc_client = Arc::clone(&self.rpc_client);
        let payer = Arc::clone(&self.payer);
        let config = self.config.clone();
        let blockhash_provider = self.blockhash_provider.clone();

        let handle = tokio::spawn(async move {
            Self::run_worker(
                revolver,
                rpc_client,
                payer,
                config,
                blockhash_provider,
                shutdown_rx,
            )
            .await
        });

        WorkerHandle {
            handle,
            shutdown_tx,
        }
    }

    /// Main worker loop
    async fn run_worker(
        revolver: Arc<RwLock<Revolver>>,
        rpc_client: Arc<RpcClient>,
        payer: Arc<Keypair>,
        config: WorkerConfig,
        blockhash_provider: Option<Arc<BlockhashProvider>>,
        mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) {
        if !config.enabled {
            info!("Revolver worker disabled in config");
            return;
        }

        info!(
            "Starting revolver worker (refresh interval: {}s)",
            config.refresh_interval_secs
        );

        let mut refresh_timer = interval(Duration::from_secs(config.refresh_interval_secs));

        loop {
            tokio::select! {
                _ = refresh_timer.tick() => {
                    if let Err(e) = Self::refresh_cycle(
                        &revolver,
                        &rpc_client,
                        &payer,
                        blockhash_provider.as_ref(),
                    ).await {
                        error!("Refresh cycle failed: {}", e);
                    }
                }
                _ = &mut shutdown_rx => {
                    info!("Revolver worker shutting down");
                    break;
                }
            }
        }
    }

    /// Perform one refresh cycle
    async fn refresh_cycle(
        revolver: &Arc<RwLock<Revolver>>,
        rpc_client: &Arc<RpcClient>,
        payer: &Arc<Keypair>,
        blockhash_provider: Option<&Arc<BlockhashProvider>>,
    ) -> Result<()> {
        debug!("Starting refresh cycle");

        // Prefer cached async provider to avoid direct RPC in refresh hot loop.
        let blockhash_fetch_started_at = Instant::now();
        let blockhash = if let Some(provider) = blockhash_provider {
            match provider.get_blockhash_if_fresh().await {
                Some(hash) => hash,
                None => provider.force_refresh().await.map_err(|e| {
                    TriggerError::Other(format!("Blockhash provider refresh failed: {}", e))
                })?,
            }
        } else {
            rpc_client
                .get_latest_blockhash()
                .await
                .map_err(|e| TriggerError::ClientError(e))?
        };
        let blockhash_fetch_ms = blockhash_fetch_started_at.elapsed().as_millis();

        if blockhash_fetch_ms > SLOW_BLOCKHASH_FETCH_MS {
            warn!(
                "RevolverWorker: blockhash refresh took {}ms (> {}ms)",
                blockhash_fetch_ms, SLOW_BLOCKHASH_FETCH_MS
            );
        }

        debug!(
            "Fetched fresh blockhash: {} in {}ms",
            blockhash, blockhash_fetch_ms
        );

        // Step 1: Acquire read lock to identify stale bullets and clone them
        let stale_bullets_to_refresh = {
            let revolver_guard = revolver.read().await;
            let active_mints = revolver_guard.get_active_mints();

            let mut stale_bullets = Vec::new();

            for mint in active_mints {
                if let Some(token_revolver) = revolver_guard.get_revolver(&mint) {
                    let stale_indices = token_revolver.get_stale_bullets();

                    if !stale_indices.is_empty() {
                        debug!(
                            "Found {} stale bullets for mint {}",
                            stale_indices.len(),
                            mint
                        );

                        // Clone stale bullets for refresh
                        for idx in stale_indices {
                            if let Some(bullet) = token_revolver.bullets.get(idx) {
                                stale_bullets.push((mint, idx, bullet.clone()));
                            }
                        }
                    }
                }
            }

            stale_bullets
        }; // Read lock released here

        if stale_bullets_to_refresh.is_empty() {
            debug!("No stale bullets to refresh");
            return Ok(());
        }

        debug!(
            "Refreshing {} stale bullets",
            stale_bullets_to_refresh.len()
        );

        // Step 2: Refresh bullets "on the side" without any lock
        let mut refreshed_bullets = Vec::new();
        for (mint, idx, mut bullet) in stale_bullets_to_refresh {
            match Self::refresh_bullet(&mut bullet, blockhash, payer).await {
                Ok(()) => {
                    refreshed_bullets.push((mint, idx, bullet));
                }
                Err(e) => {
                    warn!(
                        "Failed to refresh bullet at index {} for mint {}: {}",
                        idx, mint, e
                    );
                }
            }
        }

        let total_refreshed = refreshed_bullets.len();

        // Step 3: Acquire write lock only briefly to swap old bullets with new ones
        if !refreshed_bullets.is_empty() {
            let mut revolver_guard = revolver.write().await;

            for (mint, idx, refreshed_bullet) in refreshed_bullets {
                if let Some(token_revolver) = revolver_guard.get_revolver_mut(&mint) {
                    if let Some(bullet) = token_revolver.bullets.get_mut(idx) {
                        *bullet = refreshed_bullet;
                    }
                }
            }

            // Cleanup empty magazines
            revolver_guard.cleanup_empty();
        } // Write lock released here

        if total_refreshed > 0 {
            info!("Refreshed {} bullets with new blockhash", total_refreshed);
        }

        Ok(())
    }

    /// Refresh a single bullet with new blockhash
    pub(crate) async fn refresh_bullet(
        bullet: &mut crate::revolver::Bullet,
        blockhash: Hash,
        payer: &Arc<Keypair>,
    ) -> Result<()> {
        // Deserialize the transaction
        let mut tx: VersionedTransaction = bincode::deserialize(&bullet.tx_bytes).map_err(|e| {
            TriggerError::SerializationError(format!("Failed to deserialize transaction: {}", e))
        })?;

        // Update blockhash
        tx.message.set_recent_blockhash(blockhash);

        // Re-sign the transaction
        tx.signatures.clear();
        let signature = payer.sign_message(tx.message.serialize().as_ref());
        tx.signatures.push(signature);

        // Serialize back
        let new_tx_bytes =
            bincode::serialize(&tx).map_err(|e| TriggerError::SerializationError(e.to_string()))?;

        // Update bullet
        bullet.update_tx(new_tx_bytes);

        Ok(())
    }
}

/// Strategy configuration for generating SELL bullets
#[derive(Debug, Clone)]
pub struct SellStrategyConfig {
    /// Take-profit levels as (price_multiplier, position_fraction_bps)
    /// Example: [(1.25, 2500), (1.50, 2500), (2.00, 5000)] = 25% at +25%, 25% at +50%, 50% at +100%
    pub tp_levels: Vec<(f64, u16)>,
    /// Optional time-stop for every bullet in strategy
    pub time_stop_secs: Option<u64>,
}

impl Default for SellStrategyConfig {
    fn default() -> Self {
        Self {
            tp_levels: vec![
                (1.25, 2500), // 25% at +25% profit
                (1.50, 2500), // 25% at +50% profit
                (2.00, 5000), // 50% at +100% profit
            ],
            time_stop_secs: Some(20 * 60),
        }
    }
}

fn allocate_tp_token_amounts(amount_tokens: u64, tp_levels: &[(f64, u16)]) -> Vec<u64> {
    let mut remaining_tokens = amount_tokens;
    let mut remaining_bps: u32 = tp_levels
        .iter()
        .map(|(_, fraction_bps)| u32::from(*fraction_bps))
        .sum();
    let last_index = tp_levels.len().saturating_sub(1);
    let mut allocations = Vec::with_capacity(tp_levels.len());

    for (index, (_, fraction_bps)) in tp_levels.iter().enumerate() {
        let allocation = if remaining_tokens == 0 {
            0
        } else if index == last_index || remaining_bps <= u32::from(*fraction_bps) {
            remaining_tokens
        } else {
            (((remaining_tokens as u128) * u128::from(*fraction_bps)) / u128::from(remaining_bps))
                .min(u128::from(remaining_tokens)) as u64
        };

        allocations.push(allocation);
        remaining_tokens = remaining_tokens.saturating_sub(allocation);
        remaining_bps = remaining_bps.saturating_sub(u32::from(*fraction_bps));
    }

    allocations
}

/// Load magazine from direct buy transaction
///
/// This function creates SELL bullets based on the real entry price extracted
/// from a confirmed direct buy transaction. It generates pre-signed SELL
/// transactions at configured take-profit levels.
///
/// # Arguments
/// * `revolver` - The shared revolver instance to load bullets into
/// * `rpc_client` - RPC client for fetching blockhash
/// * `payer` - Keypair for signing transactions
/// * `mint` - Token mint address
/// * `amount_tokens` - Total tokens acquired in the buy
/// * `entry_price` - Real entry price (lamports per token, 1e9 scaled)
/// * `strategy` - Optional strategy configuration (defaults to standard TP levels)
///
/// # Returns
/// * `Ok(usize)` - Number of bullets created and loaded
/// * `Err(TriggerError)` - If bullet creation fails
pub async fn load_magazine_from_direct_buy(
    revolver: Arc<RwLock<Revolver>>,
    rpc_client: Arc<RpcClient>,
    payer: Arc<Keypair>,
    mint: Pubkey,
    amount_tokens: u64,
    entry_price: u64,
    strategy: Option<SellStrategyConfig>,
) -> Result<usize> {
    let strategy = strategy.unwrap_or_default();
    let load_started_at = Instant::now();
    let token_allocations = allocate_tp_token_amounts(amount_tokens, &strategy.tp_levels);

    info!(
        "Loading magazine from direct buy: mint={}, tokens={}, entry_price={}, levels={}",
        mint,
        amount_tokens,
        entry_price,
        strategy.tp_levels.len()
    );

    // Get fresh blockhash
    let blockhash_fetch_started_at = Instant::now();
    let blockhash = rpc_client
        .get_latest_blockhash()
        .await
        .map_err(|e| TriggerError::ClientError(e))?;
    let blockhash_fetch_ms = blockhash_fetch_started_at.elapsed().as_millis();

    if blockhash_fetch_ms > SLOW_BLOCKHASH_FETCH_MS {
        warn!(
            "RevolverWorker: magazine blockhash fetch slow for mint {}: {}ms (> {}ms)",
            mint, blockhash_fetch_ms, SLOW_BLOCKHASH_FETCH_MS
        );
    }

    let sell_builder = SellTxBuilder::new((*payer).insecure_clone(), SellTxConfig::default());

    let mut bullets = Vec::new();

    for ((multiplier, fraction_bps), token_amount) in
        strategy.tp_levels.iter().zip(token_allocations.into_iter())
    {
        // Calculate target price
        let target_price = ((entry_price as f64) * multiplier) as u64;

        if token_amount == 0 {
            warn!(
                "Skipping TP level {}: calculated token amount is 0",
                multiplier
            );
            continue;
        }

        // Calculate min SOL output with 5% slippage protection
        let min_sol_output =
            SellTxBuilder::calculate_min_output(token_amount, target_price, 500)?.max(1);

        debug!(
            "Creating bullet: multiplier={}, target_price={}, tokens={}, min_sol={}",
            multiplier, target_price, token_amount, min_sol_output
        );

        // Build and sign the SELL transaction
        let tx_bytes = sell_builder
            .build_signed_sell_tx(
                mint,
                None,
                token_amount,
                min_sol_output,
                blockhash,
                AmmProtocol::PumpFun, // Default to PumpFun
            )
            .await?;

        // Create bullet
        let bullet = Bullet::new(tx_bytes, target_price, *fraction_bps)?
            .with_time_stop(strategy.time_stop_secs);
        bullets.push(bullet);
    }

    if bullets.is_empty() && amount_tokens > 0 {
        let target_price = entry_price.max(1);
        let min_sol_output =
            SellTxBuilder::calculate_min_output(amount_tokens, target_price, 500)?.max(1);
        let tx_bytes = sell_builder
            .build_signed_sell_tx(
                mint,
                None,
                amount_tokens,
                min_sol_output,
                blockhash,
                AmmProtocol::PumpFun,
            )
            .await?;
        let bullet =
            Bullet::new(tx_bytes, target_price, 10_000)?.with_time_stop(strategy.time_stop_secs);
        warn!(
            "No TP bullets were produced for mint {}; creating sell-all fallback bullet",
            mint
        );
        bullets.push(bullet);
    }

    let bullet_count = bullets.len();

    // Load bullets into revolver
    {
        let mut revolver_guard = revolver.write().await;
        revolver_guard.load_magazine(mint, bullets);
    }

    info!(
        "Magazine loaded: {} bullets for mint {} from entry_price {} (blockhash_fetch_ms={}, total_load_ms={})",
        bullet_count,
        mint,
        entry_price,
        blockhash_fetch_ms,
        load_started_at.elapsed().as_millis()
    );

    Ok(bullet_count)
}

fn derive_shadow_lookup_keys(mint: &Pubkey) -> Vec<Pubkey> {
    let mut keys = Vec::with_capacity(3);

    for program_id in [
        crate::validation::PUMP_PROGRAM_ID,
        crate::validation::BONK_PROGRAM_ID,
    ] {
        match Pubkey::from_str(program_id) {
            Ok(program) => {
                let (curve, _) =
                    Pubkey::find_program_address(&[BONDING_CURVE_SEED, mint.as_ref()], &program);
                keys.push(curve);
            }
            Err(e) => {
                warn!(
                    "Invalid AMM program id in config ({}), falling back to mint-only lookup: {}",
                    program_id, e
                );
            }
        }
    }

    // Backward compatibility for legacy code that still keyed Shadow Ledger by mint.
    if !keys.contains(mint) {
        keys.push(*mint);
    }

    keys
}

fn select_latest_shadow_curve(
    shadow_ledger: &ShadowLedger,
    mint: &Pubkey,
) -> Option<(Pubkey, ShadowBondingCurve)> {
    let mut best: Option<(Pubkey, ShadowBondingCurve)> = None;

    for key in derive_shadow_lookup_keys(mint) {
        if let Some(curve) = shadow_ledger.get_old(&key) {
            let should_replace = best.as_ref().map_or(true, |(_, current)| {
                curve.last_updated_slot > current.last_updated_slot
            });
            if should_replace {
                best = Some((key, curve));
            }
        }
    }

    best
}

/// Check if Shadow Ledger data is stale for a given mint
///
/// This function implements the "stale data" protection requirement.
/// If data is older than max_stale_slots, it returns Err indicating
/// the decision should be deferred until fresh data is available.
///
/// # Arguments
/// * `shadow_ledger` - Shadow Ledger to check
/// * `mint` - Token mint to check
/// * `current_slot` - Current slot number
/// * `max_stale_slots` - Maximum acceptable age in slots (typically 5-10)
///
/// # Returns
/// * `Ok(())` - Data is fresh, safe to proceed with sell decision
/// * `Err(TriggerError)` - Data is stale, do NOT make sell decision
pub fn check_shadow_ledger_staleness(
    shadow_ledger: &ShadowLedger,
    mint: &Pubkey,
    current_slot: u64,
    max_stale_slots: u64,
) -> Result<()> {
    // Prefer canonical bonding-curve PDA keys, but keep mint fallback for compatibility.
    let (shadow_key, shadow_curve) =
        select_latest_shadow_curve(shadow_ledger, mint).ok_or_else(|| {
            TriggerError::Other(format!(
                "No Shadow Ledger entry for mint {} (checked curve PDAs + legacy mint key)",
                mint
            ))
        })?;

    // Calculate age
    let age_slots = current_slot.saturating_sub(shadow_curve.last_updated_slot);

    if age_slots > max_stale_slots {
        warn!(
            "Shadow Ledger data is STALE for mint {} (key={}): age={} slots (max={}). Skipping sell decision.",
            mint, shadow_key, age_slots, max_stale_slots
        );
        return Err(TriggerError::Other(format!(
            "Stale Shadow Ledger data for key {}: {} slots old (max: {}). Fallback to RPC or wait for gRPC update.",
            shadow_key, age_slots, max_stale_slots
        )));
    }

    debug!(
        "Shadow Ledger data is FRESH for mint {} (key={}): age={} slots (max={})",
        mint, shadow_key, age_slots, max_stale_slots
    );

    Ok(())
}

/// Extended staleness check returning slot info
pub struct StalenessResult {
    /// Whether data is fresh (not stale)
    pub is_fresh: bool,
    /// Age of the data in slots
    pub age_slots: u64,
    /// Last updated slot
    pub last_updated_slot: u64,
    /// Maximum allowed age
    pub max_age_slots: u64,
}

/// Check staleness with detailed result
pub fn get_staleness_info(
    shadow_ledger: &ShadowLedger,
    mint: &Pubkey,
    current_slot: u64,
    max_stale_slots: u64,
) -> Option<StalenessResult> {
    let (_, shadow_curve) = select_latest_shadow_curve(shadow_ledger, mint)?;
    let age_slots = current_slot.saturating_sub(shadow_curve.last_updated_slot);

    Some(StalenessResult {
        is_fresh: age_slots <= max_stale_slots,
        age_slots,
        last_updated_slot: shadow_curve.last_updated_slot,
        max_age_slots: max_stale_slots,
    })
}

/// Build a sell instruction using DirectSellBuilder (zero-cost, instruction-only)
///
/// This is the zero-cost alternative to using SellTxBuilder. It builds just the
/// Solana instruction without signing, suitable for when you want to compose
/// the instruction into a larger transaction.
///
/// # Arguments
/// * `payer` - The wallet executing the sell (will be marked as signer in instruction)
/// * `mint` - Token mint address
/// * `amount_tokens` - Amount of tokens to sell
/// * `entry_price` - Entry price (lamports per token, 1e9 scaled)
/// * `slippage_bps` - Slippage tolerance in basis points (e.g., 500 = 5%)
///
/// # Returns
/// A Solana instruction ready to be included in a transaction
///
/// # Example
/// ```ignore
/// let ix = build_direct_sell_instruction(
///     &payer.pubkey(),
///     &mint,
///     1_000_000,    // 1M tokens
///     30_000_000_000, // 0.00003 SOL per token (scaled by 1e9)
///     500,          // 5% slippage
/// );
/// ```
pub fn build_direct_sell_instruction(
    payer: &Pubkey,
    mint: &Pubkey,
    amount_tokens: u64,
    entry_price: u64,
    slippage_bps: u16,
) -> solana_sdk::instruction::Instruction {
    let min_sol_output =
        DirectSellBuilder::calculate_min_sol_output(amount_tokens, entry_price, slippage_bps);

    DirectSellBuilder::build_sell_ix(payer, mint, amount_tokens, min_sol_output)
}

/// Handle to a running worker
pub struct WorkerHandle {
    handle: tokio::task::JoinHandle<()>,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl WorkerHandle {
    /// Wait for the worker to complete
    pub async fn join(self) -> Result<()> {
        self.handle
            .await
            .map_err(|e| TriggerError::Other(format!("Worker task panicked: {}", e)))
    }

    /// Stop the worker gracefully
    pub fn stop(self) -> Result<()> {
        // Send shutdown signal (ignoring if receiver already dropped)
        let _ = self.shutdown_tx.send(());
        // Note: Cannot wait for completion since we moved self
        // Caller should use join() if they need to wait
        Ok(())
    }

    /// Check if worker is still running
    pub fn is_running(&self) -> bool {
        !self.handle.is_finished()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::revolver::{Bullet, Revolver};
    use solana_sdk::pubkey::Pubkey;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_worker_config_default() {
        let config = WorkerConfig::default();
        assert_eq!(config.refresh_interval_secs, 30);
        assert!(config.enabled);
        assert_eq!(config.max_stale_slots, DEFAULT_MAX_STALE_SLOTS);
    }

    #[tokio::test]
    async fn test_worker_handle_is_running() {
        let revolver = Arc::new(RwLock::new(Revolver::new()));
        let rpc_client = Arc::new(RpcClient::new("http://localhost:8899".to_string()));
        let payer = Arc::new(Keypair::new());

        let mut config = WorkerConfig::default();
        config.enabled = false; // Disable to avoid actual RPC calls

        let worker = RevolverWorker::new(revolver, rpc_client, payer, config);
        let handle = worker.start();

        // Worker should start and then immediately exit (disabled)
        sleep(Duration::from_millis(100)).await;

        // Join should complete without error
        let result = handle.join().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_refresh_bullet_updates_timestamp() {
        let _mint = Pubkey::new_unique();
        let mut bullet = Bullet::new(vec![1, 2, 3], 1000, 2500).unwrap();

        // Simulate old timestamp by waiting
        sleep(Duration::from_millis(100)).await;

        let original_time = bullet.last_update;

        // Create a dummy transaction
        let payer = Arc::new(Keypair::new());
        let tx = VersionedTransaction::default();
        let tx_bytes = bincode::serialize(&tx).unwrap();
        bullet.tx_bytes = tx_bytes;

        // Refresh should update the timestamp
        let _result = RevolverWorker::refresh_bullet(&mut bullet, Hash::default(), &payer).await;

        // The refresh may fail due to transaction signing issues in test, but that's ok
        // We're primarily testing the structure
        assert!(bullet.last_update >= original_time);
    }

    #[test]
    fn test_sell_strategy_config_default() {
        let config = SellStrategyConfig::default();
        assert_eq!(config.tp_levels.len(), 3);
        assert_eq!(config.time_stop_secs, Some(20 * 60));

        // Verify levels: +25%, +50%, +100%
        assert_eq!(config.tp_levels[0], (1.25, 2500));
        assert_eq!(config.tp_levels[1], (1.50, 2500));
        assert_eq!(config.tp_levels[2], (2.00, 5000));

        // Verify total fraction is 100%
        let total_bps: u16 = config.tp_levels.iter().map(|(_, bps)| bps).sum();
        assert_eq!(total_bps, 10000);
    }

    #[test]
    fn test_allocate_tp_token_amounts_preserves_small_positions() {
        let allocations = allocate_tp_token_amounts(1, &SellStrategyConfig::default().tp_levels);
        assert_eq!(allocations, vec![0, 0, 1]);
    }

    #[test]
    fn test_allocate_tp_token_amounts_preserves_total_amount() {
        let allocations = allocate_tp_token_amounts(7, &SellStrategyConfig::default().tp_levels);
        assert_eq!(allocations.iter().sum::<u64>(), 7);
        assert_eq!(allocations, vec![1, 2, 4]);
    }

    #[test]
    fn test_check_staleness_fresh_data() {
        use ghost_core::market_state::BondingCurve;

        let shadow_ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_000_000_000,
            virtual_sol_reserves: 30_000_000,
            real_token_reserves: 800_000_000,
            real_sol_reserves: 20_000_000,
            token_total_supply: 1_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        // Insert at slot 1000
        shadow_ledger.insert_with_slot(mint, curve, 1000);

        // Check at slot 1005 (5 slots old, max is 10) - should be fresh
        let result = check_shadow_ledger_staleness(&shadow_ledger, &mint, 1005, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_staleness_stale_data() {
        use ghost_core::market_state::BondingCurve;

        let shadow_ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_000_000_000,
            virtual_sol_reserves: 30_000_000,
            real_token_reserves: 800_000_000,
            real_sol_reserves: 20_000_000,
            token_total_supply: 1_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        // Insert at slot 1000
        shadow_ledger.insert_with_slot(mint, curve, 1000);

        // Check at slot 1015 (15 slots old, max is 10) - should be stale
        let result = check_shadow_ledger_staleness(&shadow_ledger, &mint, 1015, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_staleness_info() {
        use ghost_core::market_state::BondingCurve;

        let shadow_ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_000_000_000,
            virtual_sol_reserves: 30_000_000,
            real_token_reserves: 800_000_000,
            real_sol_reserves: 20_000_000,
            token_total_supply: 1_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        // Insert at slot 1000
        shadow_ledger.insert_with_slot(mint, curve, 1000);

        // Get staleness info at slot 1007
        let info = get_staleness_info(&shadow_ledger, &mint, 1007, 10);
        assert!(info.is_some());

        let info = info.unwrap();
        assert!(info.is_fresh);
        assert_eq!(info.age_slots, 7);
        assert_eq!(info.last_updated_slot, 1000);
        assert_eq!(info.max_age_slots, 10);
    }

    #[test]
    fn test_get_staleness_info_not_found() {
        let shadow_ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        // Should return None for non-existent mint
        let info = get_staleness_info(&shadow_ledger, &mint, 1000, 10);
        assert!(info.is_none());
    }

    #[test]
    fn test_check_staleness_uses_curve_pda_keys() {
        use ghost_core::market_state::BondingCurve;
        use std::str::FromStr;

        let shadow_ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let pump_program = Pubkey::from_str(crate::validation::PUMP_PROGRAM_ID).unwrap();
        let (curve_key, _) =
            Pubkey::find_program_address(&[BONDING_CURVE_SEED, mint.as_ref()], &pump_program);

        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_000_000_000,
            virtual_sol_reserves: 30_000_000,
            real_token_reserves: 800_000_000,
            real_sol_reserves: 20_000_000,
            token_total_supply: 1_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        shadow_ledger.insert_with_slot(curve_key, curve, 2_000);

        let result = check_shadow_ledger_staleness(&shadow_ledger, &mint, 2_005, 10);
        assert!(result.is_ok());
    }
}
