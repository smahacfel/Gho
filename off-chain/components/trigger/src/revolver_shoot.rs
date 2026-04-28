//! Revolver Shoot - Fire Bullets at Target Price
//!
//! This module provides functions to check price signals and fire bullets
//! when target prices are reached. It handles transaction submission with
//! proper error handling and metrics.
//!
//! # Usage
//!
//! ```ignore
//! // Check and shoot bullets for a specific token
//! let fired = shoot_at_price(
//!     &mut revolver,
//!     mint,
//!     current_price,
//!     &tpu_client,
//! ).await?;
//!
//! println!("Fired {} bullets", fired.len());
//! ```

use crate::errors::{Result, TriggerError};
use crate::revolver::{Bullet, Revolver};
use crate::udp_client::TpuClient;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

/// Result of a bullet shot
#[derive(Debug, Clone)]
pub struct ShotResult {
    /// The bullet that was fired
    pub bullet: Bullet,
    /// Transaction signature if successful
    pub signature: Option<String>,
    /// Error if the shot failed
    pub error: Option<String>,
}

impl ShotResult {
    /// Check if the shot was successful
    pub fn is_success(&self) -> bool {
        self.signature.is_some() && self.error.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotEventStage {
    Submitted,
    Filled,
}

#[derive(Debug, Clone)]
pub struct ShotEvent {
    pub order_id: String,
    pub stage: ShotEventStage,
    pub mint: Pubkey,
    pub candidate_id: Option<String>,
    pub position_id: Option<String>,
    pub target_price: u64,
    pub fraction_bps: u16,
    pub observed_price: u64,
    pub attempted_at_ms: u64,
    pub signature: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ShotContext {
    pub candidate_id: String,
    pub position_id: String,
}

pub trait ShotEventSink: Send + Sync {
    fn on_shot(&self, event: ShotEvent);
}

static SHOT_EVENT_SINK: OnceLock<RwLock<Option<Arc<dyn ShotEventSink>>>> = OnceLock::new();
static SHOT_CONTEXTS: OnceLock<RwLock<HashMap<Pubkey, ShotContext>>> = OnceLock::new();
static SHOT_ORDER_SEQ: AtomicU64 = AtomicU64::new(1);

fn shot_event_sink() -> &'static RwLock<Option<Arc<dyn ShotEventSink>>> {
    SHOT_EVENT_SINK.get_or_init(|| RwLock::new(None))
}

fn shot_contexts() -> &'static RwLock<HashMap<Pubkey, ShotContext>> {
    SHOT_CONTEXTS.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn set_shot_event_sink(sink: Option<Arc<dyn ShotEventSink>>) {
    if let Ok(mut guard) = shot_event_sink().write() {
        *guard = sink;
    }
}

pub fn register_shot_context(mint: Pubkey, candidate_id: String, position_id: String) {
    if let Ok(mut guard) = shot_contexts().write() {
        guard.insert(
            mint,
            ShotContext {
                candidate_id,
                position_id,
            },
        );
    }
}

pub fn unregister_shot_context(mint: &Pubkey) {
    if let Ok(mut guard) = shot_contexts().write() {
        guard.remove(mint);
    }
}

pub(crate) fn next_shot_order_id(prefix: &str) -> String {
    let seq = SHOT_ORDER_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{seq}")
}

pub(crate) fn build_shot_event(
    order_id: String,
    stage: ShotEventStage,
    mint: Pubkey,
    target_price: u64,
    fraction_bps: u16,
    observed_price: u64,
    attempted_at_ms: u64,
    signature: Option<String>,
    error: Option<String>,
) -> ShotEvent {
    let (candidate_id, position_id) = if let Ok(guard) = shot_contexts().read() {
        if let Some(ctx) = guard.get(&mint) {
            (
                Some(ctx.candidate_id.clone()),
                Some(ctx.position_id.clone()),
            )
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    ShotEvent {
        order_id,
        stage,
        mint,
        candidate_id,
        position_id,
        target_price,
        fraction_bps,
        observed_price,
        attempted_at_ms,
        signature,
        error,
    }
}

pub(crate) fn emit_shot_event(event: ShotEvent) {
    let sink = shot_event_sink()
        .read()
        .ok()
        .and_then(|guard| guard.as_ref().cloned());
    if let Some(sink) = sink {
        sink.on_shot(event);
    }
}

/// Check price and shoot bullets for a specific token
pub async fn shoot_at_price(
    revolver: &mut Revolver,
    mint: Pubkey,
    current_price: u64,
    tpu_client: &TpuClient,
) -> Result<Vec<ShotResult>> {
    debug!(
        "Checking targets for mint {} at price {}",
        mint, current_price
    );

    // Check which bullets should fire
    let bullet_indices = {
        let token_revolver = match revolver.get_revolver_mut(&mint) {
            Some(r) => r,
            None => {
                return Err(TriggerError::Other(format!(
                    "No revolver found for mint: {}",
                    mint
                )));
            }
        };
        token_revolver.check_targets(current_price)
    };

    if bullet_indices.is_empty() {
        debug!("No bullets triggered for mint {}", mint);
        return Ok(vec![]);
    }

    info!(
        "Firing {} bullets for mint {} at price {}",
        bullet_indices.len(),
        mint,
        current_price
    );

    // Take bullets to fire
    let bullets = {
        let token_revolver = match revolver.get_revolver_mut(&mint) {
            Some(r) => r,
            None => {
                return Err(TriggerError::Other(format!(
                    "No revolver found for mint: {}",
                    mint
                )));
            }
        };
        token_revolver.take_bullets(&bullet_indices)
    };

    // Fire each bullet
    let mut results = Vec::new();
    let mut failed_bullets = Vec::new();
    let mut send_fail_inc = 0u32;
    let mut relax_inc = 0u32;
    let now_ms = current_unix_ms();
    revolver.record_sell_attempt_by_mint(&mint, now_ms);
    for mut bullet in bullets {
        let order_id = next_shot_order_id("exit-live-organic");
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
        let result = fire_bullet(&bullet, tpu_client).await;
        emit_shot_event(build_shot_event(
            order_id,
            ShotEventStage::Filled,
            mint,
            bullet.target_price,
            bullet.position_fraction_bps,
            current_price,
            now_ms,
            result.signature.clone(),
            result.error.clone(),
        ));
        if !result.is_success() {
            send_fail_inc = send_fail_inc.saturating_add(1);
            match bullet.prepare_requeue() {
                Ok(()) => {
                    failed_bullets.push(bullet);
                    relax_inc = relax_inc.saturating_add(1);
                }
                Err(e) => warn!(
                    "Dropping bullet after failed shot for mint {} (target_price={}): {}",
                    mint, result.bullet.target_price, e
                ),
            }
        }
        results.push(result);
    }

    if !failed_bullets.is_empty() {
        if let Some(token_revolver) = revolver.get_revolver_mut(&mint) {
            for bullet in failed_bullets {
                token_revolver.add_bullet(bullet);
            }
        }
        warn!(
            "Re-queued failed bullets for mint {} for next retry cycle",
            mint
        );
    }
    if send_fail_inc > 0 {
        revolver.record_send_fail_by_mint(&mint, send_fail_inc);
    }
    if relax_inc > 0 {
        revolver.record_relax_by_mint(&mint, relax_inc);
    }

    // Count successes
    let success_count = results.iter().filter(|r| r.is_success()).count();
    info!(
        "Fired {} bullets for mint {}, {} successful",
        results.len(),
        mint,
        success_count
    );

    Ok(results)
}

/// Check all tokens and shoot bullets where price targets are met
pub async fn shoot_all_targets(
    revolver: &mut Revolver,
    price_oracle: &dyn PriceOracle,
    tpu_client: &TpuClient,
) -> Result<Vec<(Pubkey, Vec<ShotResult>)>> {
    let active_mints = revolver.get_active_mints();
    let mut all_results = Vec::new();

    for mint in active_mints {
        // Get current price
        let current_price = match price_oracle.get_price(&mint).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Failed to get price for mint {}: {}", mint, e);
                continue;
            }
        };

        // Shoot bullets
        match shoot_at_price(revolver, mint, current_price, tpu_client).await {
            Ok(results) => {
                if !results.is_empty() {
                    all_results.push((mint, results));
                }
            }
            Err(e) => {
                error!("Failed to shoot bullets for mint {}: {}", mint, e);
            }
        }
    }

    Ok(all_results)
}

/// Fire a single bullet
async fn fire_bullet(bullet: &Bullet, tpu_client: &TpuClient) -> ShotResult {
    // Send pre-serialized transaction directly without deserializing
    match tpu_client.send_wire_transaction(&bullet.tx_bytes).await {
        Ok(signature) => {
            info!(
                "Bullet fired successfully: signature={}, target_price={}, fraction_bps={}",
                signature, bullet.target_price, bullet.position_fraction_bps
            );
            ShotResult {
                bullet: bullet.clone(),
                signature: Some(signature.to_string()),
                error: None,
            }
        }
        Err(e) => {
            error!(
                "Failed to fire bullet at target_price={}: {}",
                bullet.target_price, e
            );
            ShotResult {
                bullet: bullet.clone(),
                signature: None,
                error: Some(e.to_string()),
            }
        }
    }
}

/// Manual shoot - fire specific bullets by index
pub async fn shoot_bullets_by_index(
    revolver: &mut Revolver,
    mint: Pubkey,
    indices: &[usize],
    tpu_client: &TpuClient,
) -> Result<Vec<ShotResult>> {
    debug!(
        "Manually shooting {} bullets for mint {}",
        indices.len(),
        mint
    );

    // Take bullets to fire
    let bullets = {
        let token_revolver = match revolver.get_revolver_mut(&mint) {
            Some(r) => r,
            None => {
                return Err(TriggerError::Other(format!(
                    "No revolver found for mint: {}",
                    mint
                )));
            }
        };
        token_revolver.take_bullets(indices)
    };

    if bullets.is_empty() {
        return Ok(vec![]);
    }

    // Fire each bullet
    let mut results = Vec::new();
    let mut failed_bullets = Vec::new();
    let mut send_fail_inc = 0u32;
    let mut relax_inc = 0u32;
    let now_ms = current_unix_ms();
    revolver.record_sell_attempt_by_mint(&mint, now_ms);
    for mut bullet in bullets {
        let order_id = next_shot_order_id("exit-live-organic");
        emit_shot_event(build_shot_event(
            order_id.clone(),
            ShotEventStage::Submitted,
            mint,
            bullet.target_price,
            bullet.position_fraction_bps,
            0,
            now_ms,
            None,
            None,
        ));
        if bullet.is_time_expired() {
            warn!(
                "Time-stop triggered for mint {} at target_price={}, forcing manual sell",
                mint, bullet.target_price
            );
        }
        let result = fire_bullet(&bullet, tpu_client).await;
        emit_shot_event(build_shot_event(
            order_id,
            ShotEventStage::Filled,
            mint,
            bullet.target_price,
            bullet.position_fraction_bps,
            0,
            now_ms,
            result.signature.clone(),
            result.error.clone(),
        ));
        if !result.is_success() {
            send_fail_inc = send_fail_inc.saturating_add(1);
            match bullet.prepare_requeue() {
                Ok(()) => {
                    failed_bullets.push(bullet);
                    relax_inc = relax_inc.saturating_add(1);
                }
                Err(e) => warn!(
                    "Dropping manually fired bullet for mint {} (target_price={}): {}",
                    mint, result.bullet.target_price, e
                ),
            }
        }
        results.push(result);
    }

    if !failed_bullets.is_empty() {
        if let Some(token_revolver) = revolver.get_revolver_mut(&mint) {
            for bullet in failed_bullets {
                token_revolver.add_bullet(bullet);
            }
        }
        warn!("Re-queued failed manually fired bullets for mint {}", mint);
    }
    if send_fail_inc > 0 {
        revolver.record_send_fail_by_mint(&mint, send_fail_inc);
    }
    if relax_inc > 0 {
        revolver.record_relax_by_mint(&mint, relax_inc);
    }

    Ok(results)
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

/// Trait for price oracles to integrate with shooting logic
#[async_trait::async_trait]
pub trait PriceOracle {
    /// Get current price for a mint in lamports
    async fn get_price(&self, mint: &Pubkey) -> Result<u64>;
}

/// Simple in-memory price oracle for testing
pub struct MockPriceOracle {
    prices: std::collections::HashMap<Pubkey, u64>,
}

impl MockPriceOracle {
    pub fn new() -> Self {
        Self {
            prices: std::collections::HashMap::new(),
        }
    }

    pub fn set_price(&mut self, mint: Pubkey, price: u64) {
        self.prices.insert(mint, price);
    }
}

#[async_trait::async_trait]
impl PriceOracle for MockPriceOracle {
    async fn get_price(&self, mint: &Pubkey) -> Result<u64> {
        self.prices
            .get(mint)
            .copied()
            .ok_or_else(|| TriggerError::Other(format!("No price for mint: {}", mint)))
    }
}

impl Default for MockPriceOracle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::revolver::{Bullet, Revolver};
    use solana_sdk::signature::Keypair;
    use solana_sdk::transaction::VersionedTransaction;

    fn create_test_bullet(target_price: u64, fraction_bps: u16) -> Bullet {
        // Create a minimal transaction for testing
        let _payer = Keypair::new();
        let tx = VersionedTransaction::default();
        let tx_bytes = bincode::serialize(&tx).unwrap();

        Bullet::new(tx_bytes, target_price, fraction_bps).unwrap()
    }

    #[test]
    fn test_shot_result_is_success() {
        let bullet = create_test_bullet(1000, 2500);

        let success = ShotResult {
            bullet: bullet.clone(),
            signature: Some("test_sig".to_string()),
            error: None,
        };
        assert!(success.is_success());

        let failure = ShotResult {
            bullet: bullet.clone(),
            signature: None,
            error: Some("error".to_string()),
        };
        assert!(!failure.is_success());
    }

    #[test]
    fn test_mock_price_oracle() {
        let mut oracle = MockPriceOracle::new();
        let mint = Pubkey::new_unique();

        oracle.set_price(mint, 5000);

        // Test in async context would require tokio runtime
        // This just tests the data structure
        assert!(oracle.prices.contains_key(&mint));
        assert_eq!(*oracle.prices.get(&mint).unwrap(), 5000);
    }

    #[tokio::test]
    async fn test_mock_price_oracle_get_price() {
        let mut oracle = MockPriceOracle::new();
        let mint = Pubkey::new_unique();

        oracle.set_price(mint, 5000);

        let price = oracle.get_price(&mint).await.unwrap();
        assert_eq!(price, 5000);
    }

    #[tokio::test]
    async fn test_mock_price_oracle_missing_price() {
        let oracle = MockPriceOracle::new();
        let mint = Pubkey::new_unique();

        let result = oracle.get_price(&mint).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_shoot_at_price_no_revolver() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let mut revolver = Revolver::new();
            let mint = Pubkey::new_unique();
            let rpc_url = "http://localhost:8899";
            let tpu_client = TpuClient::new(rpc_url.to_string(), Some(1)).unwrap();

            let result = shoot_at_price(&mut revolver, mint, 1000, &tpu_client).await;
            assert!(result.is_err());
        });
    }

    #[tokio::test]
    async fn test_shoot_at_price_requeues_failed_bullet() {
        let mut revolver = Revolver::new();
        let mint = Pubkey::new_unique();
        // Invalid wire format (too short to contain signature) forces send failure.
        let bullet = Bullet::new(vec![1, 2, 3], 1_000, 2_500).unwrap();
        revolver.load_magazine(mint, vec![bullet]);

        let tpu_client = TpuClient::new("http://localhost:8899".to_string(), Some(1)).unwrap();
        let results = shoot_at_price(&mut revolver, mint, 1_500, &tpu_client)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert!(!results[0].is_success());
        assert_eq!(revolver.total_bullet_count(), 1);
    }
}
