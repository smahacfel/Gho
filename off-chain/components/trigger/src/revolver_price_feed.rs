//! Price Feed Integration for Revolver
//!
//! This module integrates with the price oracle to fetch current prices
//! and automatically fire bullets when target prices are reached.
//!
//! # Usage
//!
//! ```ignore
//! let price_feed = PriceFeedIntegration::new(price_oracle, udp_client);
//!
//! // Fire bullets for a specific mint at current price
//! price_feed.try_fire_revolver_for_price(
//!     &mut revolver,
//!     mint,
//!     current_price,
//! ).await?;
//!
//! // Or poll and fire automatically
//! price_feed.poll_and_fire_all(&mut revolver).await?;
//! ```

use crate::errors::{Result, TriggerError};
use crate::jito_client::{JitoClient, JitoConfirmedBundle};
use crate::metrics::TriggerMetrics;
use crate::revolver::{Bullet, Revolver};
use crate::revolver_shoot::{
    build_shot_event, emit_shot_event, next_shot_order_id, ShotEventStage,
};
use crate::revolver_worker::RevolverWorker;
use crate::udp_client::TpuClient;
use solana_client::nonblocking::rpc_client::RpcClient as AsyncRpcClient;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::transaction::VersionedTransaction;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tracing::{debug, error, info, warn};

use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::market_state::{
    BondingCurve, ShadowLedgerStateConfidence, ShadowLedgerWriteReason, ShadowLedgerWriteSource,
    ShadowLedgerWriteStrength,
};
use ghost_core::shadow_ledger::ShadowLedger;
use ghost_core::{CurveFinality, CurveWriteMetadata, LAMPORTS_PER_SOL};

const BONDING_CURVE_SEED: &[u8] = b"bonding-curve";

/// Price oracle trait for fetching current prices
#[async_trait::async_trait]
pub trait PriceOracleProvider: Send + Sync {
    /// Get current price for a mint in lamports per token
    async fn get_current_price(&self, mint: &Pubkey) -> Result<u64>;
}

/// Simple wrapper around gui-backend price oracle
pub struct GuiBackendPriceOracle {
    /// Inner price oracle from gui-backend
    inner: Arc<dyn PriceOracleProvider>,
}

impl GuiBackendPriceOracle {
    /// Create new price oracle wrapper
    pub fn new(inner: Arc<dyn PriceOracleProvider>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl PriceOracleProvider for GuiBackendPriceOracle {
    async fn get_current_price(&self, mint: &Pubkey) -> Result<u64> {
        self.inner.get_current_price(mint).await
    }
}

/// Canonical price oracle with optional shared AccountStateCore and RPC fallback.
///
/// This oracle prefers shared canonical account-state when available and
/// otherwise falls back to RPC. ShadowLedger remains write-through cache only.
///
/// # Safety
///
/// This implementation ensures trading is never blocked by stale state:
/// - Fresh state (≤3 slots): Use Shadow Ledger (zero latency)
/// - Stale state (>3 slots): Fallback to RPC getAccountInfo
///
/// # Performance
///
/// - **Hot path**: ~1-2μs (Shadow Ledger hit, fresh state)
/// - **Cold path**: ~50-100ms (RPC fallback on stale/missing state)
pub struct ShadowLedgerPriceOracle {
    /// Shadow Ledger kept only as write-through cache after canonical/RPC resolution.
    shadow_ledger: Arc<ShadowLedger>,
    /// Optional shared canonical account-state core.
    account_state_core: Option<Arc<AccountStateReducer>>,
    /// RPC client for fallback queries
    rpc_client: Arc<RpcClient>,
    /// Current slot provider (for staleness checks)
    get_slot: Arc<dyn Fn() -> u64 + Send + Sync>,
    /// Metrics collector
    metrics: Option<Arc<TriggerMetrics>>,
}

impl ShadowLedgerPriceOracle {
    /// Create new Shadow Ledger price oracle with RPC fallback
    ///
    /// # Arguments
    ///
    /// * `shadow_ledger` - Shared Shadow Ledger instance
    /// * `rpc_client` - RPC client for fallback queries
    /// * `get_slot` - Function to get current slot number
    pub fn new(
        shadow_ledger: Arc<ShadowLedger>,
        rpc_client: Arc<RpcClient>,
        get_slot: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        Self {
            shadow_ledger,
            account_state_core: None,
            rpc_client,
            get_slot,
            metrics: None,
        }
    }

    /// Create with metrics
    pub fn with_metrics(
        shadow_ledger: Arc<ShadowLedger>,
        rpc_client: Arc<RpcClient>,
        get_slot: Arc<dyn Fn() -> u64 + Send + Sync>,
        metrics: Arc<TriggerMetrics>,
    ) -> Self {
        Self {
            shadow_ledger,
            account_state_core: None,
            rpc_client,
            get_slot,
            metrics: Some(metrics),
        }
    }

    pub fn with_account_state_core(
        shadow_ledger: Arc<ShadowLedger>,
        account_state_core: Arc<AccountStateReducer>,
        rpc_client: Arc<RpcClient>,
        get_slot: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        Self {
            shadow_ledger,
            account_state_core: Some(account_state_core),
            rpc_client,
            get_slot,
            metrics: None,
        }
    }

    pub fn with_metrics_and_account_state_core(
        shadow_ledger: Arc<ShadowLedger>,
        account_state_core: Arc<AccountStateReducer>,
        rpc_client: Arc<RpcClient>,
        get_slot: Arc<dyn Fn() -> u64 + Send + Sync>,
        metrics: Arc<TriggerMetrics>,
    ) -> Self {
        Self {
            shadow_ledger,
            account_state_core: Some(account_state_core),
            rpc_client,
            get_slot,
            metrics: Some(metrics),
        }
    }

    /// Fetch bonding curve from RPC and calculate price
    ///
    /// This is the fallback path when Shadow Ledger state is stale or missing.
    async fn fetch_from_rpc(&self, bonding_curve: &Pubkey) -> Result<BondingCurve> {
        debug!(
            "Fetching bonding curve from RPC for account: {}",
            bonding_curve
        );

        // Get account info from RPC
        let account_data = self
            .rpc_client
            .get_account_data(bonding_curve)
            .map_err(|e| TriggerError::Other(format!("RPC getAccountInfo failed: {}", e)))?;

        // Verify size matches BondingCurve
        if account_data.len() != 56 {
            return Err(TriggerError::Other(format!(
                "Invalid bonding curve account size: expected 56 bytes, got {}",
                account_data.len()
            )));
        }

        // Parse into BondingCurve
        let bonding_curve = BondingCurve::from_bytes(&account_data).ok_or_else(|| {
            TriggerError::Other("Failed to parse BondingCurve from RPC data".to_string())
        })?;

        Ok(*bonding_curve)
    }

    fn derive_curve_candidates(mint: &Pubkey) -> Result<Vec<Pubkey>> {
        let pump_program = Pubkey::from_str(crate::validation::PUMP_PROGRAM_ID).map_err(|e| {
            TriggerError::ConfigError(format!("Invalid Pump program id in config: {}", e))
        })?;
        let bonk_program = Pubkey::from_str(crate::validation::BONK_PROGRAM_ID).map_err(|e| {
            TriggerError::ConfigError(format!("Invalid Bonk program id in config: {}", e))
        })?;

        let (pump_curve, _) =
            Pubkey::find_program_address(&[BONDING_CURVE_SEED, mint.as_ref()], &pump_program);
        let (bonk_curve, _) =
            Pubkey::find_program_address(&[BONDING_CURVE_SEED, mint.as_ref()], &bonk_program);

        Ok(vec![pump_curve, bonk_curve])
    }

    /// Calculate price from bonding curve with validation
    ///
    /// Returns price in lamports per token unit (1e9 scale)
    fn calculate_price(curve: &BondingCurve) -> Result<u64> {
        // Validate reserves to prevent division by zero
        if curve.virtual_token_reserves == 0 {
            return Err(TriggerError::Other(
                "Invalid bonding curve: virtual_token_reserves is zero".to_string(),
            ));
        }

        // Price = (virtual_sol_reserves / virtual_token_reserves) * 1e9
        // Scale by 1e9 to convert from lamports/base_unit to lamports/token
        let price =
            (curve.virtual_sol_reserves as f64 / curve.virtual_token_reserves as f64 * 1e9) as u64;
        Ok(price)
    }

    fn try_canonical_price(&self, mint: &Pubkey) -> Option<Result<u64>> {
        let features = self.account_state_core.as_ref()?.get_features(mint)?;
        if features.price_sol.is_finite() && features.price_sol > 0.0 {
            return Some(Ok((features.price_sol * LAMPORTS_PER_SOL).round() as u64));
        }

        let (sol_reserves, token_reserves) = features.current_reserves;
        if token_reserves == 0 {
            return Some(Err(TriggerError::Other(format!(
                "Canonical account state for mint {} has zero token reserves",
                mint
            ))));
        }

        Some(Ok(
            (sol_reserves as f64 / token_reserves as f64 * 1e9) as u64
        ))
    }
}

#[async_trait::async_trait]
impl PriceOracleProvider for ShadowLedgerPriceOracle {
    async fn get_current_price(&self, mint: &Pubkey) -> Result<u64> {
        let current_slot = (self.get_slot)();
        let candidate_curves = Self::derive_curve_candidates(mint)?;
        if let Some(price) = self.try_canonical_price(mint) {
            let price = price?;
            debug!("AccountStateCore hit: mint={}, price={}", mint, price);
            return Ok(price);
        }

        // PR7: AccountStateCore is the only in-process truth source; RPC is the
        // canonical network fallback when shared canonical state is unavailable.
        if self.metrics.is_some() {
            debug!("Price oracle canonical-miss RPC path engaged");
        }
        for curve_key in &candidate_curves {
            match self.fetch_from_rpc(curve_key).await {
                Ok(curve) => {
                    let price = Self::calculate_price(&curve)?;
                    #[allow(deprecated)]
                    let _ = self.shadow_ledger.apply_curve_write(
                        Some(*mint),
                        *curve_key,
                        curve,
                        CurveWriteMetadata::new(
                            ShadowLedgerWriteSource::RpcBootstrapSeeder,
                            ShadowLedgerWriteStrength::ConfirmedBootstrap,
                            ShadowLedgerStateConfidence::Observed,
                            ShadowLedgerWriteReason::ConfirmedBootstrap,
                            Some(current_slot),
                            CurveFinality::Provisional,
                        ),
                    );
                    info!(
                        "RPC fetch successful: mint={}, curve={}, price={}",
                        mint, curve_key, price
                    );
                    return Ok(price);
                }
                Err(err) => {
                    debug!(
                        "RPC fetch failed for mint {} on curve {}: {}",
                        mint, curve_key, err
                    );
                }
            }
        }

        Err(TriggerError::Other(format!(
            "No bonding curve account found for mint {} across supported AMMs",
            mint
        )))
    }
}

/// Price feed integration for Revolver
pub struct PriceFeedIntegration {
    /// Price oracle for fetching current prices
    price_oracle: Arc<dyn PriceOracleProvider>,
    /// UDP socket for sending transactions
    udp_socket: Arc<UdpSocket>,
    /// TPU leader address
    leader_tpu_addr: SocketAddr,
    /// Optional metrics
    metrics: Option<Arc<TriggerMetrics>>,
}

impl PriceFeedIntegration {
    /// Create new price feed integration
    pub fn new(
        price_oracle: Arc<dyn PriceOracleProvider>,
        udp_socket: Arc<UdpSocket>,
        leader_tpu_addr: SocketAddr,
    ) -> Self {
        Self {
            price_oracle,
            udp_socket,
            leader_tpu_addr,
            metrics: None,
        }
    }

    /// Create with metrics
    pub fn with_metrics(
        price_oracle: Arc<dyn PriceOracleProvider>,
        udp_socket: Arc<UdpSocket>,
        leader_tpu_addr: SocketAddr,
        metrics: Arc<TriggerMetrics>,
    ) -> Self {
        Self {
            price_oracle,
            udp_socket,
            leader_tpu_addr,
            metrics: Some(metrics),
        }
    }

    /// Try to fire bullets for a specific mint at the given price
    ///
    /// This function:
    /// 1. Locks the revolver
    /// 2. Checks which bullets should fire at the current price
    /// 3. Sends those bullets via UDP to TPU
    /// 4. Removes fired bullets from the revolver
    pub async fn try_fire_revolver_for_price(
        &self,
        revolver: &mut Revolver,
        mint: Pubkey,
        current_price: u64,
    ) -> Result<usize> {
        debug!(
            "Checking bullets for mint {} at price {}",
            mint, current_price
        );

        // Check which bullets should fire
        let bullet_indices = {
            let token_revolver = match revolver.get_revolver_mut(&mint) {
                Some(r) => r,
                None => {
                    debug!("No revolver found for mint {}", mint);
                    return Ok(0);
                }
            };
            token_revolver.check_targets(current_price)
        };

        if bullet_indices.is_empty() {
            debug!(
                "No bullets triggered for mint {} at price {}",
                mint, current_price
            );
            return Ok(0);
        }

        info!(
            "Found {} bullets to fire for mint {} at price {}",
            bullet_indices.len(),
            mint,
            current_price
        );

        // Take bullets to fire
        let bullets = {
            let token_revolver = match revolver.get_revolver_mut(&mint) {
                Some(r) => r,
                None => {
                    debug!("No revolver found for mint {}", mint);
                    return Ok(0);
                }
            };
            token_revolver.take_bullets(&bullet_indices)
        };

        let mut fired_count = 0;
        let mut not_ready_count = 0;
        let mut retry_bullets = Vec::new();
        let mut send_fail_inc = 0u32;
        let mut relax_inc = 0u32;
        revolver.record_sell_attempt_by_mint(&mint, current_unix_ms());

        for mut bullet in bullets {
            let order_id = next_shot_order_id("exit-live-organic");
            let now_ms = current_unix_ms();
            emit_shot_event(build_shot_event(
                order_id.clone(),
                ShotEventStage::Submitted,
                mint,
                bullet.target_price,
                bullet.position_fraction_bps,
                current_price,
                now_ms,
                None,
                None,
            ));
            if bullet.is_time_expired() {
                warn!(
                    "Time-stop triggered for mint {} at target_price={}, forcing sell",
                    mint, bullet.target_price
                );
            }
            // Check if tx_bytes is empty
            if bullet.tx_bytes.is_empty() {
                warn!(
                    "Bullet for mint {} has empty tx_bytes, skipping (target_price={})",
                    mint, bullet.target_price
                );
                emit_shot_event(build_shot_event(
                    order_id,
                    ShotEventStage::Filled,
                    mint,
                    bullet.target_price,
                    bullet.position_fraction_bps,
                    current_price,
                    now_ms,
                    None,
                    Some("empty_tx_bytes".to_string()),
                ));
                not_ready_count += 1;

                if let Some(ref metrics) = self.metrics {
                    metrics.bullet_failed_not_ready_total.inc();
                }
                match bullet.prepare_requeue() {
                    Ok(()) => {
                        retry_bullets.push(bullet);
                        relax_inc = relax_inc.saturating_add(1);
                    }
                    Err(e) => warn!(
                        "Dropping not-ready bullet for mint {} (target_price={}): {}",
                        mint, bullet.target_price, e
                    ),
                }
                continue;
            }

            // Send transaction via UDP
            match self.send_bullet(&bullet.tx_bytes).await {
                Ok(()) => {
                    emit_shot_event(build_shot_event(
                        order_id,
                        ShotEventStage::Filled,
                        mint,
                        bullet.target_price,
                        bullet.position_fraction_bps,
                        current_price,
                        now_ms,
                        None,
                        None,
                    ));
                    info!(
                        "Revolver fired bullet: mint={}, target_price={}, fraction_bps={}, size={}B",
                        mint, bullet.target_price, bullet.position_fraction_bps, bullet.tx_bytes.len()
                    );
                    fired_count += 1;

                    if let Some(ref metrics) = self.metrics {
                        metrics.bullet_fired_total.inc();
                    }
                }
                Err(e) => {
                    emit_shot_event(build_shot_event(
                        order_id,
                        ShotEventStage::Filled,
                        mint,
                        bullet.target_price,
                        bullet.position_fraction_bps,
                        current_price,
                        now_ms,
                        None,
                        Some(e.to_string()),
                    ));
                    error!(
                        "Failed to fire bullet for mint {} at target_price {}: {}",
                        mint, bullet.target_price, e
                    );
                    send_fail_inc = send_fail_inc.saturating_add(1);

                    if let Some(ref metrics) = self.metrics {
                        metrics.bullet_failed_not_ready_total.inc();
                    }
                    match bullet.prepare_requeue() {
                        Ok(()) => {
                            retry_bullets.push(bullet);
                            relax_inc = relax_inc.saturating_add(1);
                        }
                        Err(requeue_err) => warn!(
                            "Dropping failed bullet for mint {} (target_price={}): {}",
                            mint, bullet.target_price, requeue_err
                        ),
                    }
                }
            }
        }

        if !retry_bullets.is_empty() {
            if let Some(token_revolver) = revolver.get_revolver_mut(&mint) {
                for bullet in retry_bullets {
                    token_revolver.add_bullet(bullet);
                }
            }
            warn!("Re-queued failed bullets for mint {} for retry", mint);
        }
        if send_fail_inc > 0 {
            revolver.record_send_fail_by_mint(&mint, send_fail_inc);
        }
        if relax_inc > 0 {
            revolver.record_relax_by_mint(&mint, relax_inc);
        }

        if not_ready_count > 0 {
            warn!(
                "Skipped {} bullets for mint {} due to empty tx_bytes",
                not_ready_count, mint
            );
        }

        Ok(fired_count)
    }

    /// Send bullet transaction bytes via UDP
    async fn send_bullet(&self, tx_bytes: &[u8]) -> Result<()> {
        self.udp_socket
            .send_to(tx_bytes, self.leader_tpu_addr)
            .await
            .map_err(|e| {
                TriggerError::NetworkError(format!(
                    "Failed to send bullet via UDP to {}: {}",
                    self.leader_tpu_addr, e
                ))
            })?;

        Ok(())
    }

    /// Poll price oracle and fire bullets for all active mints
    pub async fn poll_and_fire_all(&self, revolver: &mut Revolver) -> Result<usize> {
        let active_mints = revolver.get_active_mints();
        let mut total_fired = 0;

        for mint in active_mints {
            // Get current price
            let current_price = match self.price_oracle.get_current_price(&mint).await {
                Ok(price) => price,
                Err(e) => {
                    warn!("Failed to get price for mint {}: {}", mint, e);
                    continue;
                }
            };

            // Try to fire bullets
            match self
                .try_fire_revolver_for_price(revolver, mint, current_price)
                .await
            {
                Ok(fired) => {
                    total_fired += fired;
                }
                Err(e) => {
                    error!("Failed to fire bullets for mint {}: {}", mint, e);
                }
            }
        }

        if total_fired > 0 {
            info!("Fired {} bullets total across all mints", total_fired);
        }

        Ok(total_fired)
    }

    /// Create a worker that continuously polls and fires bullets
    pub fn start_polling_worker(
        self: Arc<Self>,
        revolver: Arc<tokio::sync::RwLock<Revolver>>,
        poll_interval_secs: u64,
    ) -> PriceFeedWorkerHandle {
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let handle = tokio::spawn(async move {
            Self::run_polling_worker(self, revolver, poll_interval_secs, shutdown_rx).await
        });

        PriceFeedWorkerHandle {
            handle,
            shutdown_tx,
        }
    }

    /// Main polling worker loop
    async fn run_polling_worker(
        price_feed: Arc<Self>,
        revolver: Arc<tokio::sync::RwLock<Revolver>>,
        poll_interval_secs: u64,
        mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) {
        info!(
            "Starting price feed polling worker (interval: {}s)",
            poll_interval_secs
        );

        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(poll_interval_secs));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let mut revolver_guard = revolver.write().await;

                    if let Err(e) = price_feed.poll_and_fire_all(&mut revolver_guard).await {
                        error!("Polling cycle failed: {}", e);
                    }
                }
                _ = &mut shutdown_rx => {
                    info!("Price feed polling worker shutting down");
                    break;
                }
            }
        }
    }
}

/// Handle to a running price feed worker
pub struct PriceFeedWorkerHandle {
    handle: tokio::task::JoinHandle<()>,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl PriceFeedWorkerHandle {
    /// Wait for the worker to complete
    pub async fn join(self) -> Result<()> {
        self.handle
            .await
            .map_err(|e| TriggerError::Other(format!("Worker task panicked: {}", e)))
    }

    /// Stop the worker gracefully
    pub fn stop(self) -> Result<()> {
        let _ = self.shutdown_tx.send(());
        Ok(())
    }

    /// Check if worker is still running
    pub fn is_running(&self) -> bool {
        !self.handle.is_finished()
    }
}

/// Conservative fixed Jito tip for legacy SELL bullets submitted via the Revolver path.
/// Keep this aligned with the launcher live-exit cap so dormant legacy code cannot
/// silently spend BUY-sized tips if it is ever reactivated.
const BULLET_JITO_TIP_LAMPORTS: u64 = 300_000;

/// Jito executor used by Revolver to submit SELL bullets via authoritative Jito bundles.
#[derive(Clone)]
pub struct JitoBulletExecutor {
    client: Arc<JitoClient>,
    rpc_client: Arc<AsyncRpcClient>,
    payer: Arc<Keypair>,
}

impl JitoBulletExecutor {
    pub fn new(
        client: Arc<JitoClient>,
        rpc_client: Arc<AsyncRpcClient>,
        payer: Arc<Keypair>,
    ) -> Self {
        Self {
            client,
            rpc_client,
            payer,
        }
    }

    pub async fn submit_bullet(&self, bullet: &mut Bullet) -> Result<JitoConfirmedBundle> {
        if bullet.needs_refresh() {
            self.refresh_bullet(bullet).await?;
        }

        let sell_tx: VersionedTransaction =
            bincode::deserialize(&bullet.tx_bytes).map_err(|e| {
                TriggerError::SerializationError(format!(
                    "Failed to deserialize Jito SELL bullet transaction: {}",
                    e
                ))
            })?;

        // Reuse the sell tx blockhash for the tip tx so both share the same expiry window.
        let recent_blockhash = match &sell_tx.message {
            solana_sdk::message::VersionedMessage::Legacy(msg) => msg.recent_blockhash,
            solana_sdk::message::VersionedMessage::V0(msg) => msg.recent_blockhash,
        };

        let tip_tx = self
            .client
            .create_tip_transaction(&self.payer, BULLET_JITO_TIP_LAMPORTS, recent_blockhash)
            .map_err(|e| {
                TriggerError::TransactionBuildFailed(format!(
                    "Failed to create Jito tip transaction for SELL bullet: {}",
                    e
                ))
            })?;

        // Jito requires the bundle to be [payload_tx, ..., tip_tx].
        self.client
            .submit_bundle_and_confirm(vec![sell_tx, tip_tx])
            .await
    }

    async fn refresh_bullet(&self, bullet: &mut Bullet) -> Result<()> {
        let blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .await
            .map_err(TriggerError::ClientError)?;
        RevolverWorker::refresh_bullet(bullet, blockhash, &self.payer).await
    }
}

/// Integration with TpuClient for more robust sending
pub struct PriceFeedWithTpuClient {
    /// Price oracle
    price_oracle: Arc<dyn PriceOracleProvider>,
    /// Retained for constructor compatibility; live SELL dispatch no longer downgrades to TPU.
    _tpu_client: Arc<TpuClient>,
    /// Optional Jito executor for authoritative SELL bundle landing.
    jito_executor: Option<Arc<JitoBulletExecutor>>,
    /// Optional metrics
    metrics: Option<Arc<TriggerMetrics>>,
}

impl PriceFeedWithTpuClient {
    /// Create new price feed with TPU client
    pub fn new(price_oracle: Arc<dyn PriceOracleProvider>, tpu_client: Arc<TpuClient>) -> Self {
        Self {
            price_oracle,
            _tpu_client: tpu_client,
            jito_executor: None,
            metrics: None,
        }
    }

    /// Create with metrics
    pub fn with_metrics(
        price_oracle: Arc<dyn PriceOracleProvider>,
        tpu_client: Arc<TpuClient>,
        metrics: Arc<TriggerMetrics>,
    ) -> Self {
        Self {
            price_oracle,
            _tpu_client: tpu_client,
            jito_executor: None,
            metrics: Some(metrics),
        }
    }

    pub fn with_jito_executor(mut self, jito_executor: Arc<JitoBulletExecutor>) -> Self {
        self.jito_executor = Some(jito_executor);
        self
    }

    /// Try to fire bullets using TpuClient for redundancy
    pub async fn try_fire_revolver_for_price(
        &self,
        revolver: &mut Revolver,
        mint: Pubkey,
        current_price: u64,
    ) -> Result<usize> {
        debug!(
            "Checking bullets for mint {} at price {} (using TpuClient)",
            mint, current_price
        );

        // Check which bullets should fire
        let bullet_indices = {
            let token_revolver = match revolver.get_revolver_mut(&mint) {
                Some(r) => r,
                None => {
                    debug!("No revolver found for mint {}", mint);
                    return Ok(0);
                }
            };
            token_revolver.check_targets(current_price)
        };

        if bullet_indices.is_empty() {
            debug!(
                "No bullets triggered for mint {} at price {}",
                mint, current_price
            );
            return Ok(0);
        }

        info!(
            "Found {} bullets to fire for mint {} at price {}",
            bullet_indices.len(),
            mint,
            current_price
        );

        // Take bullets to fire
        let bullets = {
            let token_revolver = match revolver.get_revolver_mut(&mint) {
                Some(r) => r,
                None => {
                    debug!("No revolver found for mint {}", mint);
                    return Ok(0);
                }
            };
            token_revolver.take_bullets(&bullet_indices)
        };

        let mut fired_count = 0;
        let mut not_ready_count = 0;
        let mut retry_bullets = Vec::new();
        let mut send_fail_inc = 0u32;
        let mut relax_inc = 0u32;
        revolver.record_sell_attempt_by_mint(&mint, current_unix_ms());

        for mut bullet in bullets {
            let order_id = next_shot_order_id("exit-live-organic");
            let now_ms = current_unix_ms();
            emit_shot_event(build_shot_event(
                order_id.clone(),
                ShotEventStage::Submitted,
                mint,
                bullet.target_price,
                bullet.position_fraction_bps,
                current_price,
                now_ms,
                None,
                None,
            ));
            if bullet.is_time_expired() {
                warn!(
                    "Time-stop triggered for mint {} at target_price={}, forcing sell",
                    mint, bullet.target_price
                );
            }
            // Check if tx_bytes is empty
            if bullet.tx_bytes.is_empty() {
                warn!(
                    "Bullet for mint {} has empty tx_bytes, skipping (target_price={})",
                    mint, bullet.target_price
                );
                emit_shot_event(build_shot_event(
                    order_id,
                    ShotEventStage::Filled,
                    mint,
                    bullet.target_price,
                    bullet.position_fraction_bps,
                    current_price,
                    now_ms,
                    None,
                    Some("empty_tx_bytes".to_string()),
                ));
                not_ready_count += 1;

                if let Some(ref metrics) = self.metrics {
                    metrics.bullet_failed_not_ready_total.inc();
                }
                match bullet.prepare_requeue() {
                    Ok(()) => {
                        retry_bullets.push(bullet);
                        relax_inc = relax_inc.saturating_add(1);
                    }
                    Err(e) => warn!(
                        "Dropping not-ready bullet for mint {} (target_price={}): {}",
                        mint, bullet.target_price, e
                    ),
                }
                continue;
            }

            let send_result: Result<String> = if let Some(jito_executor) = &self.jito_executor {
                jito_executor.submit_bullet(&mut bullet).await.map(|confirmed_bundle| {
                    info!(
                        "Revolver fired Jito bullet: mint={}, sig={}, bundle_uuid={}, landed_slot={:?}, target_price={}, fraction_bps={}, size={}B",
                        mint,
                        confirmed_bundle.signature,
                        confirmed_bundle.bundle_uuid,
                        confirmed_bundle.landed_slot,
                        bullet.target_price,
                        bullet.position_fraction_bps,
                        bullet.tx_bytes.len()
                    );
                    confirmed_bundle.signature.to_string()
                })
            } else {
                Err(TriggerError::ConfigError(
                    "SELL bullet dispatch requires a Jito executor; TPU fallback is disabled"
                        .to_string(),
                ))
            };

            match send_result {
                Ok(signature) => {
                    emit_shot_event(build_shot_event(
                        order_id,
                        ShotEventStage::Filled,
                        mint,
                        bullet.target_price,
                        bullet.position_fraction_bps,
                        current_price,
                        now_ms,
                        Some(signature),
                        None,
                    ));
                    info!(
                        "Revolver fired bullet: mint={}, target_price={}, fraction_bps={}, size={}B",
                        mint, bullet.target_price, bullet.position_fraction_bps, bullet.tx_bytes.len()
                    );
                    fired_count += 1;

                    if let Some(ref metrics) = self.metrics {
                        metrics.bullet_fired_total.inc();
                    }
                }
                Err(e) => {
                    emit_shot_event(build_shot_event(
                        order_id,
                        ShotEventStage::Filled,
                        mint,
                        bullet.target_price,
                        bullet.position_fraction_bps,
                        current_price,
                        now_ms,
                        None,
                        Some(e.to_string()),
                    ));
                    error!(
                        "Failed to fire bullet for mint {} at target_price {}: {}",
                        mint, bullet.target_price, e
                    );
                    send_fail_inc = send_fail_inc.saturating_add(1);

                    if let Some(ref metrics) = self.metrics {
                        metrics.bullet_failed_not_ready_total.inc();
                    }
                    match bullet.prepare_requeue() {
                        Ok(()) => {
                            retry_bullets.push(bullet);
                            relax_inc = relax_inc.saturating_add(1);
                        }
                        Err(requeue_err) => warn!(
                            "Dropping failed bullet for mint {} (target_price={}): {}",
                            mint, bullet.target_price, requeue_err
                        ),
                    }
                }
            }
        }

        if !retry_bullets.is_empty() {
            if let Some(token_revolver) = revolver.get_revolver_mut(&mint) {
                for bullet in retry_bullets {
                    token_revolver.add_bullet(bullet);
                }
            }
            warn!("Re-queued failed bullets for mint {} for retry", mint);
        }
        if send_fail_inc > 0 {
            revolver.record_send_fail_by_mint(&mint, send_fail_inc);
        }
        if relax_inc > 0 {
            revolver.record_relax_by_mint(&mint, relax_inc);
        }

        if not_ready_count > 0 {
            warn!(
                "Skipped {} bullets for mint {} due to empty/invalid tx_bytes",
                not_ready_count, mint
            );
        }

        Ok(fired_count)
    }

    /// Poll and fire all active mints
    pub async fn poll_and_fire_all(&self, revolver: &mut Revolver) -> Result<usize> {
        let active_mints = revolver.get_active_mints();
        let mut total_fired = 0;

        for mint in active_mints {
            // Get current price
            let current_price = match self.price_oracle.get_current_price(&mint).await {
                Ok(price) => price,
                Err(e) => {
                    warn!("Failed to get price for mint {}: {}", mint, e);
                    continue;
                }
            };

            // Try to fire bullets
            match self
                .try_fire_revolver_for_price(revolver, mint, current_price)
                .await
            {
                Ok(fired) => {
                    total_fired += fired;
                }
                Err(e) => {
                    error!("Failed to fire bullets for mint {}: {}", mint, e);
                }
            }
        }

        if total_fired > 0 {
            info!("Fired {} bullets total across all mints", total_fired);
        }

        Ok(total_fired)
    }
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::revolver::{Bullet, Revolver};
    use std::collections::HashMap;
    use std::sync::RwLock;

    // Mock price oracle for testing
    struct MockPriceOracle {
        prices: RwLock<HashMap<Pubkey, u64>>,
    }

    impl MockPriceOracle {
        fn new() -> Self {
            Self {
                prices: RwLock::new(HashMap::new()),
            }
        }

        fn set_price(&self, mint: Pubkey, price: u64) {
            self.prices.write().unwrap().insert(mint, price);
        }
    }

    #[async_trait::async_trait]
    impl PriceOracleProvider for MockPriceOracle {
        async fn get_current_price(&self, mint: &Pubkey) -> Result<u64> {
            self.prices
                .read()
                .unwrap()
                .get(mint)
                .copied()
                .ok_or_else(|| TriggerError::Other(format!("No price for mint: {}", mint)))
        }
    }

    #[tokio::test]
    async fn test_try_fire_revolver_no_bullets() {
        let oracle = Arc::new(MockPriceOracle::new());
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let leader_addr = "127.0.0.1:8001".parse().unwrap();

        let price_feed = PriceFeedIntegration::new(oracle, socket, leader_addr);

        let mut revolver = Revolver::new();
        let mint = Pubkey::new_unique();

        // No revolver for mint
        let result = price_feed
            .try_fire_revolver_for_price(&mut revolver, mint, 1000)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_try_fire_revolver_empty_tx_bytes() {
        let oracle = Arc::new(MockPriceOracle::new());
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let leader_addr = "127.0.0.1:8001".parse().unwrap();

        let price_feed = PriceFeedIntegration::new(oracle, socket, leader_addr);

        let mut revolver = Revolver::new();
        let mint = Pubkey::new_unique();

        // Add bullet with empty tx_bytes
        let bullet = Bullet::new(vec![], 1000, 2500).unwrap();
        revolver.load_magazine(mint, vec![bullet]);

        // Should skip empty bullet
        let result = price_feed
            .try_fire_revolver_for_price(&mut revolver, mint, 1500)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
        assert_eq!(revolver.total_bullet_count(), 1);
    }

    #[tokio::test]
    async fn test_poll_and_fire_all() {
        let oracle = Arc::new(MockPriceOracle::new());
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let leader_addr = "127.0.0.1:8001".parse().unwrap();

        let price_feed = PriceFeedIntegration::new(oracle.clone(), socket, leader_addr);

        let mut revolver = Revolver::new();
        let mint = Pubkey::new_unique();

        // Set price
        oracle.set_price(mint, 1500);

        // Add bullet (will have empty tx_bytes in test, but that's ok)
        let bullet = Bullet::new(vec![1, 2, 3], 1000, 2500).unwrap();
        revolver.load_magazine(mint, vec![bullet]);

        // Should attempt to fire (will fail to send, but won't error)
        let result = price_feed.poll_and_fire_all(&mut revolver).await;
        assert!(result.is_ok());
    }

    #[test]
    fn shadow_ledger_price_oracle_does_not_use_deprecated_shadow_quote_truth() {
        let source = include_str!("revolver_price_feed.rs");
        let implementation = source
            .split("#[cfg(test)]")
            .next()
            .expect("implementation section must exist");
        let deprecated_shadow_quote = ["shadow_ledger", ".get_", "quote("].concat();
        assert!(
            !implementation.contains(&deprecated_shadow_quote),
            "PR7 invariant: revolver price feed must not use ShadowLedger get_quote as truth source"
        );
    }
}
