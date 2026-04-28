//! Revolver Module - SELL Bullet Management
//!
//! The Revolver manages pre-signed SELL transactions (bullets) that can be fired
//! when price targets are reached. Each bullet represents a position fraction
//! that can be sold at a specific target price.
//!
//! # Architecture
//!
//! - **Bullet**: A single pre-signed SELL transaction with target price and position fraction
//! - **TokenRevolver**: Manages all bullets for a specific token mint
//! - **Revolver**: Top-level manager for all token revolvers
//!
//! # Usage
//!
//! ```ignore
//! let mut revolver = Revolver::new();
//!
//! // Load magazine after BUY
//! revolver.load_magazine(mint, bullets);
//!
//! // Check and shoot bullets at target price
//! let fired = revolver.check_and_shoot(mint, current_price);
//! ```

use crate::control_command::{
    AemCommandApplyResult, AemCommandDirective, AemCommandPriority, ExecutionStressSnapshot,
};
use crate::errors::{Result, TriggerError};
use solana_sdk::message::VersionedMessage;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::VersionedTransaction;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_MAX_REQUEUE_ATTEMPTS: u32 = 3;
const DEFAULT_REQUEUE_SLIPPAGE_STEP_BPS: u16 = 150;
const DEFAULT_MAX_REQUEUE_SLIPPAGE_BPS: u16 = 600;

/// A single SELL bullet (pre-signed transaction)
#[derive(Debug, Clone)]
pub struct Bullet {
    /// Serialized transaction bytes ready to send
    pub tx_bytes: Vec<u8>,
    /// Target price in lamports (or internal price format)
    pub target_price: u64,
    /// Last time this bullet was updated (blockhash refresh)
    pub last_update: Instant,
    /// Position fraction in basis points (0-10000 = 0-100%)
    pub position_fraction_bps: u16,
    /// Time when bullet was created
    pub created_at: Instant,
    /// Optional time stop (seconds). If elapsed, bullet should be fired regardless of price.
    pub time_stop_secs: Option<u64>,
    /// Number of failed re-queue attempts
    pub requeue_count: u32,
}

impl Bullet {
    /// Create a new bullet
    pub fn new(tx_bytes: Vec<u8>, target_price: u64, position_fraction_bps: u16) -> Result<Self> {
        if position_fraction_bps > 10000 {
            return Err(TriggerError::ConfigError(
                "Position fraction must be between 0 and 10000 bps".to_string(),
            ));
        }

        Ok(Self {
            tx_bytes,
            target_price,
            last_update: Instant::now(),
            position_fraction_bps,
            created_at: Instant::now(),
            time_stop_secs: None,
            requeue_count: 0,
        })
    }

    /// Configure optional time-stop for this bullet
    pub fn with_time_stop(mut self, time_stop_secs: Option<u64>) -> Self {
        self.time_stop_secs = time_stop_secs;
        self
    }

    /// Check if this bullet needs a blockhash refresh
    /// Bullets older than 60 seconds should be refreshed
    pub fn needs_refresh(&self) -> bool {
        const REFRESH_THRESHOLD_SECS: u64 = 60;
        self.last_update.elapsed().as_secs() >= REFRESH_THRESHOLD_SECS
    }

    /// Check if bullet exceeded configured time stop
    pub fn is_time_expired(&self) -> bool {
        match self.time_stop_secs {
            Some(secs) => self.created_at.elapsed().as_secs() >= secs,
            None => false,
        }
    }

    /// Update the transaction bytes with a freshly signed version
    pub fn update_tx(&mut self, new_tx_bytes: Vec<u8>) {
        self.tx_bytes = new_tx_bytes;
        self.last_update = Instant::now();
    }

    /// Check if the current price triggers this bullet
    pub fn should_fire(&self, current_price: u64) -> bool {
        current_price >= self.target_price || self.is_time_expired()
    }

    /// Check whether bullet can be re-queued again
    pub fn can_requeue(&self) -> bool {
        self.requeue_count < DEFAULT_MAX_REQUEUE_ATTEMPTS
    }

    /// Prepare bullet for re-queue:
    /// - increase retry counter
    /// - relax min_output in tx to reduce infinite revert loops
    pub fn prepare_requeue(&mut self) -> Result<()> {
        if !self.can_requeue() {
            return Err(TriggerError::Other(
                "Bullet exceeded max re-queue attempts".to_string(),
            ));
        }

        self.requeue_count = self.requeue_count.saturating_add(1);

        let extra_slippage_bps = (self.requeue_count as u16)
            .saturating_mul(DEFAULT_REQUEUE_SLIPPAGE_STEP_BPS)
            .min(DEFAULT_MAX_REQUEUE_SLIPPAGE_BPS);

        if extra_slippage_bps == 0 || self.tx_bytes.is_empty() {
            return Ok(());
        }

        if let Ok(updated) = relax_min_output_in_tx(&self.tx_bytes, extra_slippage_bps) {
            self.tx_bytes = updated;
        }
        Ok(())
    }
}

fn relax_min_output_in_tx(tx_bytes: &[u8], extra_slippage_bps: u16) -> Result<Vec<u8>> {
    let mut tx: VersionedTransaction = bincode::deserialize(tx_bytes).map_err(|e| {
        TriggerError::SerializationError(format!(
            "Failed to deserialize tx for requeue min_output relaxation: {}",
            e
        ))
    })?;

    let mut patched = false;
    let slippage_factor = 10_000u128.saturating_sub(extra_slippage_bps as u128);

    let instructions = match &mut tx.message {
        VersionedMessage::Legacy(msg) => &mut msg.instructions,
        VersionedMessage::V0(msg) => &mut msg.instructions,
    };

    for ix in instructions.iter_mut() {
        if ix.data.len() < 24 {
            continue;
        }

        let old_min = u64::from_le_bytes(ix.data[16..24].try_into().map_err(|e| {
            TriggerError::SerializationError(format!("Failed to decode min_output bytes: {}", e))
        })?);
        if old_min == 0 {
            continue;
        }

        let new_min = ((old_min as u128)
            .saturating_mul(slippage_factor)
            .checked_div(10_000)
            .unwrap_or(0)
            .max(1)) as u64;

        if new_min < old_min {
            ix.data[16..24].copy_from_slice(&new_min.to_le_bytes());
            patched = true;
        }
    }

    if !patched {
        return Err(TriggerError::Other(
            "No patchable min_output found in serialized bullet transaction".to_string(),
        ));
    }

    bincode::serialize(&tx).map_err(|e| TriggerError::SerializationError(e.to_string()))
}

/// Strategy mode for position management based on Ghost Intelligence analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrategyMode {
    /// Default strategy - normal TP/SL levels
    #[default]
    Default,
    /// Panic sell - trigger immediate exit
    PanicSell,
    /// Tight stop loss - conservative exit (tighten trailing stop)
    TightStopLoss,
    /// Loose stop loss - let profits run (widen trailing stop)
    LooseStopLoss,
}

impl StrategyMode {
    /// Get human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            StrategyMode::Default => "DEFAULT",
            StrategyMode::PanicSell => "PANIC_SELL",
            StrategyMode::TightStopLoss => "TIGHT_STOP_LOSS",
            StrategyMode::LooseStopLoss => "LOOSE_STOP_LOSS",
        }
    }
}

/// Manages bullets for a single token
#[derive(Debug, Clone)]
pub struct TokenRevolver {
    /// Token mint address
    pub mint: Pubkey,
    /// List of bullets for this token, sorted by target_price (ascending)
    pub bullets: Vec<Bullet>,
    /// Quarantined bullets removed from the active firing set after terminal corruption/retry
    /// exhaustion. They remain attached to the position for operator visibility and fail-closed
    /// lifecycle accounting.
    pub quarantined_bullets: Vec<Bullet>,
    /// Strategy mode for this position (set by Ghost Intelligence)
    pub strategy_mode: StrategyMode,
    /// Stable AEM position identifier.
    pub position_id: Option<String>,
    /// Active position epoch for anti-zombie commands.
    pub position_epoch: u64,
    /// Hard-safety lock expiration (unix ms).
    pub hard_lock_until_unix_ms: u64,
    /// Freeze panic expiration (unix ms).
    pub panic_freeze_until_unix_ms: u64,
    /// Last accepted command priority.
    pub last_command_priority: Option<AemCommandPriority>,
    /// Last accepted command timestamp.
    pub last_command_issued_at_unix_ms: Option<u64>,
    /// Force immediate full exit flag.
    pub force_exit_all: bool,
    /// Force partial exit fraction (bps).
    pub force_exit_fraction_bps: Option<u16>,
    /// Stress telemetry counters.
    pub send_fail_count: u32,
    pub relax_count: u32,
    /// Last sell attempt unix ms.
    pub last_sell_attempt_unix_ms: Option<u64>,
}

impl TokenRevolver {
    /// Create a new token revolver
    pub fn new(mint: Pubkey) -> Self {
        Self {
            mint,
            bullets: Vec::new(),
            quarantined_bullets: Vec::new(),
            strategy_mode: StrategyMode::Default,
            position_id: None,
            position_epoch: 0,
            hard_lock_until_unix_ms: 0,
            panic_freeze_until_unix_ms: 0,
            last_command_priority: None,
            last_command_issued_at_unix_ms: None,
            force_exit_all: false,
            force_exit_fraction_bps: None,
            send_fail_count: 0,
            relax_count: 0,
            last_sell_attempt_unix_ms: None,
        }
    }

    /// Set strategy to Panic Sell (trigger immediate exit)
    /// Called by Ghost Intelligence when high risk is detected
    pub fn set_panic_sell(&mut self) {
        if !self.is_panic_frozen(now_unix_ms()) {
            self.strategy_mode = StrategyMode::PanicSell;
        }
    }

    /// Set strategy to Tight Stop Loss (conservative exit)
    /// Called by Ghost Intelligence when low viral score is detected
    pub fn set_tight_stop_loss(&mut self) {
        self.strategy_mode = StrategyMode::TightStopLoss;
    }

    /// Set strategy to Loose Stop Loss (let profits run)
    /// Called by Ghost Intelligence when clean dev + high viral detected
    pub fn set_loose_stop_loss(&mut self) {
        self.strategy_mode = StrategyMode::LooseStopLoss;
    }

    /// Reset strategy to default
    pub fn reset_strategy(&mut self) {
        self.strategy_mode = StrategyMode::Default;
    }

    pub fn register_position_epoch(&mut self, position_id: String, position_epoch: u64) {
        self.position_id = Some(position_id);
        self.position_epoch = position_epoch;
    }

    pub fn unregister_position_epoch(&mut self) {
        self.position_id = None;
        self.position_epoch = 0;
        self.hard_lock_until_unix_ms = 0;
        self.panic_freeze_until_unix_ms = 0;
        self.force_exit_all = false;
        self.force_exit_fraction_bps = None;
    }

    pub fn record_send_fail(&mut self) {
        self.send_fail_count = self.send_fail_count.saturating_add(1);
    }

    pub fn record_relax(&mut self) {
        self.relax_count = self.relax_count.saturating_add(1);
    }

    pub fn record_requeue_relax(&mut self, times: u32) {
        self.relax_count = self.relax_count.saturating_add(times);
    }

    pub fn record_sell_attempt(&mut self, now_unix_ms: u64) {
        self.last_sell_attempt_unix_ms = Some(now_unix_ms);
    }

    pub fn is_panic_frozen(&self, now_unix_ms: u64) -> bool {
        now_unix_ms <= self.panic_freeze_until_unix_ms
    }

    pub fn get_execution_stress(&self, now_unix_ms: u64) -> ExecutionStressSnapshot {
        ExecutionStressSnapshot {
            requeue_count: self
                .bullets
                .iter()
                .chain(self.quarantined_bullets.iter())
                .map(|b| b.requeue_count)
                .max()
                .unwrap_or(0),
            send_fail_count: self.send_fail_count,
            relax_count: self.relax_count,
            oracle_stale_age_ms: 0,
            last_sell_attempt_age_ms: self
                .last_sell_attempt_unix_ms
                .map(|ts| now_unix_ms.saturating_sub(ts)),
        }
    }

    pub fn apply_aem_control_command(
        &mut self,
        issued_at_unix_ms: u64,
        valid_from_unix_ms: u64,
        expires_at_unix_ms: u64,
        position_epoch: u64,
        priority: AemCommandPriority,
        directive: AemCommandDirective,
        _reason_code: &str,
        now_unix_ms: u64,
    ) -> AemCommandApplyResult {
        if position_epoch != self.position_epoch {
            return AemCommandApplyResult {
                accepted: false,
                reject_reason: Some("epoch_mismatch".to_string()),
            };
        }
        if now_unix_ms > expires_at_unix_ms {
            return AemCommandApplyResult {
                accepted: false,
                reject_reason: Some("command_expired".to_string()),
            };
        }
        if now_unix_ms < valid_from_unix_ms {
            return AemCommandApplyResult {
                accepted: false,
                reject_reason: Some("not_yet_valid".to_string()),
            };
        }
        if now_unix_ms <= self.hard_lock_until_unix_ms && priority < AemCommandPriority::HardSafety
        {
            return AemCommandApplyResult {
                accepted: false,
                reject_reason: Some("hard_safety_lock_active".to_string()),
            };
        }

        match directive {
            AemCommandDirective::Noop => {}
            AemCommandDirective::SetTightStop => self.set_tight_stop_loss(),
            AemCommandDirective::SetLooseStop => self.set_loose_stop_loss(),
            AemCommandDirective::ForceExitAll => {
                self.force_exit_all = true;
                if !self.is_panic_frozen(now_unix_ms) {
                    self.strategy_mode = StrategyMode::PanicSell;
                }
            }
            AemCommandDirective::ForceExitFractionBps { fraction_bps } => {
                self.force_exit_fraction_bps = Some(fraction_bps.min(10_000));
                self.strategy_mode = StrategyMode::TightStopLoss;
            }
            AemCommandDirective::FreezePanic => {
                self.panic_freeze_until_unix_ms =
                    self.panic_freeze_until_unix_ms.max(expires_at_unix_ms);
                if self.strategy_mode == StrategyMode::PanicSell {
                    self.strategy_mode = StrategyMode::TightStopLoss;
                }
            }
        }

        if priority == AemCommandPriority::HardSafety {
            self.hard_lock_until_unix_ms = self.hard_lock_until_unix_ms.max(expires_at_unix_ms);
        }
        self.last_command_priority = Some(priority);
        self.last_command_issued_at_unix_ms = Some(issued_at_unix_ms);

        AemCommandApplyResult {
            accepted: true,
            reject_reason: None,
        }
    }

    /// Check if panic sell is triggered
    pub fn is_panic_sell(&self) -> bool {
        self.strategy_mode == StrategyMode::PanicSell
    }

    /// Get current strategy mode
    pub fn get_strategy_mode(&self) -> StrategyMode {
        self.strategy_mode
    }

    /// Load bullets into the magazine
    /// Automatically sorts by target_price
    pub fn load_bullets(&mut self, mut bullets: Vec<Bullet>) {
        bullets.sort_by_key(|b| b.target_price);
        self.bullets = bullets;
    }

    /// Add a single bullet to the magazine
    pub fn add_bullet(&mut self, bullet: Bullet) {
        self.bullets.push(bullet);
        // Re-sort to maintain order
        self.bullets.sort_by_key(|b| b.target_price);
    }

    /// Move a bullet out of the active firing set into quarantine.
    pub fn quarantine_bullet(&mut self, bullet: Bullet) {
        self.quarantined_bullets.push(bullet);
    }

    /// Get bullets that need refresh
    pub fn get_stale_bullets(&self) -> Vec<usize> {
        self.bullets
            .iter()
            .enumerate()
            .filter_map(|(idx, bullet)| {
                if bullet.needs_refresh() {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check current price and return bullets that should fire
    /// Returns indices of bullets that should be fired
    pub fn check_targets(&self, current_price: u64) -> Vec<usize> {
        let now_ms = now_unix_ms();
        if self.force_exit_all
            || (self.strategy_mode == StrategyMode::PanicSell && !self.is_panic_frozen(now_ms))
        {
            return (0..self.bullets.len()).collect();
        }

        if let Some(fraction_bps) = self.force_exit_fraction_bps {
            let mut out = Vec::new();
            let mut acc = 0u16;
            for (idx, bullet) in self.bullets.iter().enumerate() {
                if acc >= fraction_bps {
                    break;
                }
                out.push(idx);
                acc = acc.saturating_add(bullet.position_fraction_bps);
            }
            if !out.is_empty() {
                return out;
            }
        }

        self.bullets
            .iter()
            .enumerate()
            .filter_map(|(idx, bullet)| {
                if bullet.should_fire(current_price) {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Remove and return bullets at specified indices
    pub fn take_bullets(&mut self, indices: &[usize]) -> Vec<Bullet> {
        // Sort indices in descending order to remove from end first
        let mut sorted_indices = indices.to_vec();
        sorted_indices.sort_by(|a, b| b.cmp(a));

        let mut taken = Vec::new();
        for idx in sorted_indices {
            if idx < self.bullets.len() {
                taken.push(self.bullets.remove(idx));
            }
        }
        taken.reverse(); // Restore original order
        taken
    }

    /// Check if magazine is empty
    pub fn is_empty(&self) -> bool {
        self.bullets.is_empty()
    }

    /// Get number of bullets remaining
    pub fn bullet_count(&self) -> usize {
        self.bullets.len()
    }

    /// Get number of quarantined bullets retained for operator intervention.
    pub fn quarantined_bullet_count(&self) -> usize {
        self.quarantined_bullets.len()
    }

    /// Get the total number of active + quarantined bullets attached to the position.
    pub fn total_artifact_count(&self) -> usize {
        self.bullet_count()
            .saturating_add(self.quarantined_bullet_count())
    }
}

/// Top-level Revolver managing all token revolvers
#[derive(Debug, Clone)]
pub struct Revolver {
    /// Map of mint -> TokenRevolver
    pub tokens: HashMap<Pubkey, TokenRevolver>,
}

impl Revolver {
    /// Create a new revolver
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
        }
    }

    /// Load a magazine for a token (replaces existing if any)
    pub fn load_magazine(&mut self, mint: Pubkey, bullets: Vec<Bullet>) {
        let mut token_revolver = TokenRevolver::new(mint);
        token_revolver.load_bullets(bullets);
        self.tokens.insert(mint, token_revolver);
    }

    pub fn register_position_epoch(&mut self, position_id: &str, position_epoch: u64) {
        if let Some(mint) = parse_mint_from_position_id(position_id) {
            if let Some(revolver) = self.tokens.get_mut(&mint) {
                revolver.register_position_epoch(position_id.to_string(), position_epoch);
                return;
            }
        }
        if let Some((_mint, revolver)) = self
            .tokens
            .iter_mut()
            .find(|(_m, r)| r.position_id.as_deref() == Some(position_id))
        {
            revolver.register_position_epoch(position_id.to_string(), position_epoch);
            return;
        }
        if let Some((_mint, revolver)) = self
            .tokens
            .iter_mut()
            .find(|(_m, r)| r.position_id.is_none())
        {
            revolver.register_position_epoch(position_id.to_string(), position_epoch);
        }
    }

    pub fn unregister_position_epoch(&mut self, position_id: &str) {
        if let Some(mint) = parse_mint_from_position_id(position_id) {
            if let Some(revolver) = self.tokens.get_mut(&mint) {
                revolver.unregister_position_epoch();
                return;
            }
        }
        if let Some((_mint, revolver)) = self
            .tokens
            .iter_mut()
            .find(|(_m, r)| r.position_id.as_deref() == Some(position_id))
        {
            revolver.unregister_position_epoch();
        }
    }

    /// Add a bullet to an existing token revolver
    pub fn add_bullet(&mut self, mint: Pubkey, bullet: Bullet) -> Result<()> {
        if let Some(revolver) = self.tokens.get_mut(&mint) {
            revolver.add_bullet(bullet);
            Ok(())
        } else {
            Err(TriggerError::Other(format!(
                "No revolver found for mint: {}",
                mint
            )))
        }
    }

    /// Move a bullet into the per-position quarantine bucket.
    pub fn quarantine_bullet(&mut self, mint: Pubkey, bullet: Bullet) -> Result<()> {
        if let Some(revolver) = self.tokens.get_mut(&mint) {
            revolver.quarantine_bullet(bullet);
            Ok(())
        } else {
            Err(TriggerError::Other(format!(
                "No revolver found for mint: {}",
                mint
            )))
        }
    }

    /// Get all mints with loaded magazines
    pub fn get_active_mints(&self) -> Vec<Pubkey> {
        self.tokens.keys().copied().collect()
    }

    /// Get a reference to a token revolver
    pub fn get_revolver(&self, mint: &Pubkey) -> Option<&TokenRevolver> {
        self.tokens.get(mint)
    }

    /// Get a mutable reference to a token revolver
    pub fn get_revolver_mut(&mut self, mint: &Pubkey) -> Option<&mut TokenRevolver> {
        self.tokens.get_mut(mint)
    }

    pub fn get_execution_stress_by_position(
        &self,
        position_id: &str,
    ) -> Option<ExecutionStressSnapshot> {
        let now_ms = now_unix_ms();
        if let Some(mint) = parse_mint_from_position_id(position_id) {
            if let Some(r) = self.tokens.get(&mint) {
                return Some(r.get_execution_stress(now_ms));
            }
        }
        self.tokens
            .values()
            .find(|r| r.position_id.as_deref() == Some(position_id))
            .map(|r| r.get_execution_stress(now_ms))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_aem_control_command(
        &mut self,
        position_id: &str,
        position_epoch: u64,
        issued_at_unix_ms: u64,
        valid_from_unix_ms: u64,
        expires_at_unix_ms: u64,
        priority: AemCommandPriority,
        directive: AemCommandDirective,
        reason_code: &str,
        now_unix_ms: u64,
    ) -> AemCommandApplyResult {
        if let Some(mint) = parse_mint_from_position_id(position_id) {
            if let Some(token_revolver) = self.tokens.get_mut(&mint) {
                if token_revolver.position_id.is_none() {
                    token_revolver.register_position_epoch(position_id.to_string(), position_epoch);
                }
                return token_revolver.apply_aem_control_command(
                    issued_at_unix_ms,
                    valid_from_unix_ms,
                    expires_at_unix_ms,
                    position_epoch,
                    priority,
                    directive,
                    reason_code,
                    now_unix_ms,
                );
            }
        }
        let Some((_mint, token_revolver)) = self
            .tokens
            .iter_mut()
            .find(|(_m, r)| r.position_id.as_deref() == Some(position_id))
        else {
            return AemCommandApplyResult {
                accepted: false,
                reject_reason: Some("position_not_found".to_string()),
            };
        };

        token_revolver.apply_aem_control_command(
            issued_at_unix_ms,
            valid_from_unix_ms,
            expires_at_unix_ms,
            position_epoch,
            priority,
            directive,
            reason_code,
            now_unix_ms,
        )
    }

    pub fn record_send_fail_by_mint(&mut self, mint: &Pubkey, count: u32) {
        if let Some(token) = self.tokens.get_mut(mint) {
            for _ in 0..count {
                token.record_send_fail();
            }
        }
    }

    pub fn record_relax_by_mint(&mut self, mint: &Pubkey, count: u32) {
        if let Some(token) = self.tokens.get_mut(mint) {
            token.record_requeue_relax(count);
        }
    }

    pub fn record_sell_attempt_by_mint(&mut self, mint: &Pubkey, now_unix_ms: u64) {
        if let Some(token) = self.tokens.get_mut(mint) {
            token.record_sell_attempt(now_unix_ms);
        }
    }

    /// Get a mutable reference to a token magazine (alias for get_revolver_mut)
    /// Used by Ghost Intelligence for strategy updates
    pub fn get_magazine_mut(&mut self, mint: &Pubkey) -> Option<&mut TokenRevolver> {
        self.get_revolver_mut(mint)
    }

    /// Remove a token revolver (e.g., when all bullets are fired)
    pub fn unload_magazine(&mut self, mint: &Pubkey) -> Option<TokenRevolver> {
        self.tokens.remove(mint)
    }

    /// Get total number of bullets across all tokens
    pub fn total_bullet_count(&self) -> usize {
        self.tokens.values().map(|r| r.bullet_count()).sum()
    }

    pub fn total_artifact_count(&self) -> usize {
        self.tokens.values().map(|r| r.total_artifact_count()).sum()
    }

    /// Clean up empty magazines
    pub fn cleanup_empty(&mut self) {
        self.tokens.retain(|_, revolver| !revolver.is_empty());
    }
}

impl Default for Revolver {
    fn default() -> Self {
        Self::new()
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn parse_mint_from_position_id(position_id: &str) -> Option<Pubkey> {
    let mut parts = position_id.split(':');
    let _pool = parts.next()?;
    let mint = parts.next()?;
    Pubkey::from_str(mint).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_bullet_creation() {
        let bullet = Bullet::new(vec![1, 2, 3], 1000, 2500).unwrap();
        assert_eq!(bullet.target_price, 1000);
        assert_eq!(bullet.position_fraction_bps, 2500);
        assert!(!bullet.needs_refresh());
    }

    #[test]
    fn test_bullet_invalid_fraction() {
        let result = Bullet::new(vec![1, 2, 3], 1000, 10001);
        assert!(result.is_err());
    }

    #[test]
    fn test_bullet_should_fire() {
        let bullet = Bullet::new(vec![1, 2, 3], 1000, 2500).unwrap();
        assert!(bullet.should_fire(1000));
        assert!(bullet.should_fire(1500));
        assert!(!bullet.should_fire(999));
    }

    #[test]
    fn test_bullet_time_stop_triggers_fire() {
        let mut bullet = Bullet::new(vec![1, 2, 3], 10_000, 2500)
            .unwrap()
            .with_time_stop(Some(60));
        bullet.created_at = Instant::now() - Duration::from_secs(61);
        assert!(bullet.is_time_expired());
        assert!(bullet.should_fire(1));
    }

    #[test]
    fn test_bullet_requeue_limit() {
        let mut bullet = Bullet::new(vec![], 1000, 2500).unwrap();
        assert!(bullet.prepare_requeue().is_ok());
        assert!(bullet.prepare_requeue().is_ok());
        assert!(bullet.prepare_requeue().is_ok());
        assert!(bullet.prepare_requeue().is_err());
    }

    #[test]
    fn test_token_revolver_load_bullets() {
        let mint = Pubkey::new_unique();
        let mut revolver = TokenRevolver::new(mint);

        let bullets = vec![
            Bullet::new(vec![1], 5000, 1000).unwrap(),
            Bullet::new(vec![2], 3000, 2000).unwrap(),
            Bullet::new(vec![3], 7000, 1500).unwrap(),
        ];

        revolver.load_bullets(bullets);
        assert_eq!(revolver.bullet_count(), 3);

        // Verify sorted by target_price
        assert_eq!(revolver.bullets[0].target_price, 3000);
        assert_eq!(revolver.bullets[1].target_price, 5000);
        assert_eq!(revolver.bullets[2].target_price, 7000);
    }

    #[test]
    fn test_token_revolver_check_targets() {
        let mint = Pubkey::new_unique();
        let mut revolver = TokenRevolver::new(mint);

        revolver.load_bullets(vec![
            Bullet::new(vec![1], 1000, 1000).unwrap(),
            Bullet::new(vec![2], 2000, 2000).unwrap(),
            Bullet::new(vec![3], 3000, 3000).unwrap(),
        ]);

        let to_fire = revolver.check_targets(2500);
        assert_eq!(to_fire.len(), 2); // Should fire bullets at 1000 and 2000
    }

    #[test]
    fn test_token_revolver_take_bullets() {
        let mint = Pubkey::new_unique();
        let mut revolver = TokenRevolver::new(mint);

        revolver.load_bullets(vec![
            Bullet::new(vec![1], 1000, 1000).unwrap(),
            Bullet::new(vec![2], 2000, 2000).unwrap(),
            Bullet::new(vec![3], 3000, 3000).unwrap(),
        ]);

        let taken = revolver.take_bullets(&[0, 2]);
        assert_eq!(taken.len(), 2);
        assert_eq!(taken[0].target_price, 1000);
        assert_eq!(taken[1].target_price, 3000);
        assert_eq!(revolver.bullet_count(), 1);
        assert_eq!(revolver.bullets[0].target_price, 2000);
    }

    #[test]
    fn test_revolver_load_magazine() {
        let mut revolver = Revolver::new();
        let mint = Pubkey::new_unique();

        let bullets = vec![
            Bullet::new(vec![1], 1000, 1000).unwrap(),
            Bullet::new(vec![2], 2000, 2000).unwrap(),
        ];

        revolver.load_magazine(mint, bullets);
        assert_eq!(revolver.get_active_mints().len(), 1);
        assert_eq!(revolver.total_bullet_count(), 2);
    }

    #[test]
    fn test_revolver_cleanup_empty() {
        let mut revolver = Revolver::new();
        let mint1 = Pubkey::new_unique();
        let mint2 = Pubkey::new_unique();

        revolver.load_magazine(mint1, vec![Bullet::new(vec![1], 1000, 1000).unwrap()]);
        revolver.load_magazine(mint2, vec![]);

        assert_eq!(revolver.tokens.len(), 2);
        revolver.cleanup_empty();
        assert_eq!(revolver.tokens.len(), 1);
        assert!(revolver.get_revolver(&mint1).is_some());
        assert!(revolver.get_revolver(&mint2).is_none());
    }

    // === StrategyMode Tests (Ghost Intelligence Integration) ===

    #[test]
    fn test_strategy_mode_default() {
        let mint = Pubkey::new_unique();
        let revolver = TokenRevolver::new(mint);
        assert_eq!(revolver.strategy_mode, StrategyMode::Default);
        assert!(!revolver.is_panic_sell());
    }

    #[test]
    fn test_strategy_mode_panic_sell() {
        let mint = Pubkey::new_unique();
        let mut revolver = TokenRevolver::new(mint);

        revolver.set_panic_sell();
        assert_eq!(revolver.strategy_mode, StrategyMode::PanicSell);
        assert!(revolver.is_panic_sell());
        assert_eq!(revolver.get_strategy_mode(), StrategyMode::PanicSell);
    }

    #[test]
    fn test_strategy_mode_tight_stop_loss() {
        let mint = Pubkey::new_unique();
        let mut revolver = TokenRevolver::new(mint);

        revolver.set_tight_stop_loss();
        assert_eq!(revolver.strategy_mode, StrategyMode::TightStopLoss);
        assert!(!revolver.is_panic_sell());
    }

    #[test]
    fn test_strategy_mode_loose_stop_loss() {
        let mint = Pubkey::new_unique();
        let mut revolver = TokenRevolver::new(mint);

        revolver.set_loose_stop_loss();
        assert_eq!(revolver.strategy_mode, StrategyMode::LooseStopLoss);
        assert!(!revolver.is_panic_sell());
    }

    #[test]
    fn test_strategy_mode_reset() {
        let mint = Pubkey::new_unique();
        let mut revolver = TokenRevolver::new(mint);

        revolver.set_panic_sell();
        assert!(revolver.is_panic_sell());

        revolver.reset_strategy();
        assert_eq!(revolver.strategy_mode, StrategyMode::Default);
        assert!(!revolver.is_panic_sell());
    }

    #[test]
    fn test_strategy_mode_name() {
        assert_eq!(StrategyMode::Default.name(), "DEFAULT");
        assert_eq!(StrategyMode::PanicSell.name(), "PANIC_SELL");
        assert_eq!(StrategyMode::TightStopLoss.name(), "TIGHT_STOP_LOSS");
        assert_eq!(StrategyMode::LooseStopLoss.name(), "LOOSE_STOP_LOSS");
    }

    #[test]
    fn test_get_magazine_mut_alias() {
        let mut revolver = Revolver::new();
        let mint = Pubkey::new_unique();

        revolver.load_magazine(mint, vec![Bullet::new(vec![1], 1000, 1000).unwrap()]);

        // Test that get_magazine_mut works the same as get_revolver_mut
        {
            let magazine = revolver.get_magazine_mut(&mint);
            assert!(magazine.is_some());
            magazine.unwrap().set_panic_sell();
        }

        // Verify the strategy was set
        let revolver_ref = revolver.get_revolver(&mint);
        assert!(revolver_ref.is_some());
        assert!(revolver_ref.unwrap().is_panic_sell());
    }

    #[test]
    fn test_apply_aem_control_command_rejects_epoch_mismatch() {
        let mint = Pubkey::new_unique();
        let position_id = format!("pool:{}:1700000000000", mint);
        let mut revolver = Revolver::new();
        revolver.load_magazine(mint, vec![Bullet::new(vec![1], 1000, 1000).unwrap()]);
        revolver.register_position_epoch(&position_id, 1);

        let out = revolver.apply_aem_control_command(
            &position_id,
            2,
            1000,
            1000,
            2000,
            AemCommandPriority::AemPolicy,
            AemCommandDirective::SetTightStop,
            "test_epoch_mismatch",
            1000,
        );

        assert!(!out.accepted);
        assert_eq!(out.reject_reason.as_deref(), Some("epoch_mismatch"));
    }

    #[test]
    fn test_apply_aem_control_command_rejects_expired_ttl() {
        let mint = Pubkey::new_unique();
        let position_id = format!("pool:{}:1700000000001", mint);
        let mut revolver = Revolver::new();
        revolver.load_magazine(mint, vec![Bullet::new(vec![1], 1000, 1000).unwrap()]);
        revolver.register_position_epoch(&position_id, 1);

        let out = revolver.apply_aem_control_command(
            &position_id,
            1,
            1000,
            1000,
            1500,
            AemCommandPriority::AemPolicy,
            AemCommandDirective::SetTightStop,
            "test_expired",
            2000,
        );

        assert!(!out.accepted);
        assert_eq!(out.reject_reason.as_deref(), Some("command_expired"));
    }

    #[test]
    fn test_apply_aem_control_command_rejects_lower_priority_under_hard_lock() {
        let mint = Pubkey::new_unique();
        let position_id = format!("pool:{}:1700000000002", mint);
        let mut revolver = Revolver::new();
        revolver.load_magazine(mint, vec![Bullet::new(vec![1], 1000, 1000).unwrap()]);
        revolver.register_position_epoch(&position_id, 1);

        let hard = revolver.apply_aem_control_command(
            &position_id,
            1,
            1000,
            1000,
            5000,
            AemCommandPriority::HardSafety,
            AemCommandDirective::ForceExitAll,
            "hard_lock",
            1000,
        );
        assert!(hard.accepted);

        let lower = revolver.apply_aem_control_command(
            &position_id,
            1,
            1200,
            1200,
            4000,
            AemCommandPriority::AemPolicy,
            AemCommandDirective::SetLooseStop,
            "lower_priority",
            1200,
        );

        assert!(!lower.accepted);
        assert_eq!(
            lower.reject_reason.as_deref(),
            Some("hard_safety_lock_active")
        );
    }
}
