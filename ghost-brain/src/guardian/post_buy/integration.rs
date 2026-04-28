//! Lane-aware post-buy runtime routing for Guardian and AEM.
//!
//! The Guardian emits lane-tagged signals, while the router fans them out to the
//! correct position-management sink:
//! - live/paper/single => real `Revolver`
//! - shadow => virtual `ShadowPositionBook`

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::time::Duration;

use solana_sdk::pubkey::Pubkey;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

use trigger::{create_virtual_magazine, DirectBuyBuilder, MagazineConfig, Revolver, TokenRevolver};

use crate::aem::{
    CommandApplyResult, CommandDirective, CommandPriority, ControlCommand, ExecutionStressSnapshot,
    TriggerControlAdapter,
};
use crate::execution::backend::Lane;

use super::signals::{GuardianSignal, RecommendedAction, SignalSeverity};

const SHADOW_PRICE_SCALE: f64 = 1_000_000_000_000_000.0;
pub(crate) const SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS: u64 = 30;

fn scale_price_to_target_key(price: f64) -> Option<u64> {
    if !price.is_finite() || price <= 0.0 {
        return None;
    }
    let scaled = (price * SHADOW_PRICE_SCALE).round();
    if !scaled.is_finite() {
        return Some(u64::MAX);
    }
    Some(scaled.clamp(1.0, u64::MAX as f64) as u64)
}

fn shadow_magazine_config() -> MagazineConfig {
    let mut config = MagazineConfig::default_targets(DirectBuyBuilder::pump_program_id());
    config.time_stop_secs = Some(SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS);
    config
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedPositionAction {
    TightenStop,
    PanicSell,
}

#[derive(Debug, Clone)]
pub struct ShadowExitExecution {
    pub position_id: String,
    pub position_epoch: u64,
    pub fraction_bps: u16,
    pub remaining_fraction_bps: u16,
    pub fill_price: f64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ShadowExitPreview {
    pub fraction_bps: u16,
    pub has_time_stop_trigger: bool,
}

#[derive(Debug, Default)]
pub struct ShadowPositionBook {
    tokens: HashMap<Pubkey, TokenRevolver>,
    position_mints: HashMap<String, Pubkey>,
}

impl ShadowPositionBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn position_count(&self) -> usize {
        self.tokens.len()
    }

    pub fn register_position(
        &mut self,
        base_mint: Pubkey,
        position_id: &str,
        position_epoch: u64,
        entry_price: f64,
    ) -> Result<(), String> {
        let Some(entry_price_key) = scale_price_to_target_key(entry_price) else {
            return Err(format!(
                "invalid shadow entry price for virtual magazine: {}",
                entry_price
            ));
        };

        if let Some(existing) = self.tokens.get(&base_mint) {
            if existing.position_id.as_deref() == Some(position_id) {
                return Ok(());
            }
        }

        let bullets = create_virtual_magazine(entry_price_key, &shadow_magazine_config())
            .map_err(|e| e.to_string())?;

        if let Some(old) = self
            .tokens
            .get(&base_mint)
            .and_then(|token| token.position_id.clone())
        {
            self.position_mints.remove(&old);
        }

        let mut token_revolver = TokenRevolver::new(base_mint);
        token_revolver.load_bullets(bullets);
        token_revolver.register_position_epoch(position_id.to_string(), position_epoch);
        self.position_mints
            .insert(position_id.to_string(), base_mint);
        self.tokens.insert(base_mint, token_revolver);
        Ok(())
    }

    pub fn has_position(&self, position_id: &str) -> bool {
        self.position_mints.contains_key(position_id)
    }

    pub fn is_position_active(&self, base_mint: &Pubkey, position_id: &str) -> bool {
        self.tokens.get(base_mint).is_some_and(|token| {
            token.position_id.as_deref() == Some(position_id) && token.total_artifact_count() > 0
        })
    }

    pub fn peek_exit_fraction_bps(&self, base_mint: &Pubkey, current_price: f64) -> u16 {
        self.preview_exit(base_mint, current_price).fraction_bps
    }

    pub fn preview_exit(&self, base_mint: &Pubkey, current_price: f64) -> ShadowExitPreview {
        let Some(current_price_key) = scale_price_to_target_key(current_price) else {
            return ShadowExitPreview::default();
        };
        let Some(token_revolver) = self.tokens.get(base_mint) else {
            return ShadowExitPreview::default();
        };

        let bullet_indices = token_revolver.check_targets(current_price_key);
        if bullet_indices.is_empty() {
            return ShadowExitPreview::default();
        }

        let forced_by_command = token_revolver.force_exit_all
            || token_revolver.force_exit_fraction_bps.is_some()
            || (token_revolver.is_panic_sell()
                && !token_revolver.is_panic_frozen(current_unix_ms()));

        let mut fraction_bps = 0u32;
        let mut has_time_stop_trigger = false;
        for idx in bullet_indices {
            let Some(bullet) = token_revolver.bullets.get(idx) else {
                continue;
            };
            fraction_bps = fraction_bps.saturating_add(bullet.position_fraction_bps as u32);
            if !forced_by_command
                && bullet.is_time_expired()
                && current_price_key < bullet.target_price
            {
                has_time_stop_trigger = true;
            }
        }

        ShadowExitPreview {
            fraction_bps: fraction_bps.min(10_000) as u16,
            has_time_stop_trigger,
        }
    }

    pub fn remove_position(&mut self, position_id: &str) -> bool {
        let Some(mint) = self.position_mints.remove(position_id) else {
            return false;
        };
        self.tokens.remove(&mint).is_some()
    }

    pub fn force_exit_all(&mut self, base_mint: &Pubkey) -> bool {
        let Some(token_revolver) = self.tokens.get_mut(base_mint) else {
            return false;
        };
        token_revolver.force_exit_all = true;
        true
    }

    pub fn refresh_time_stop_anchor(&mut self, base_mint: &Pubkey) -> bool {
        let Some(token_revolver) = self.tokens.get_mut(base_mint) else {
            return false;
        };
        let refreshed_at = Instant::now();
        for bullet in &mut token_revolver.bullets {
            bullet.created_at = refreshed_at;
        }
        true
    }

    #[cfg(test)]
    pub fn age_position_for_time_stop_for_tests(
        &mut self,
        base_mint: &Pubkey,
        age_secs: u64,
    ) -> bool {
        let Some(token_revolver) = self.tokens.get_mut(base_mint) else {
            return false;
        };
        let Some(created_at) = Instant::now().checked_sub(Duration::from_secs(age_secs)) else {
            return false;
        };
        for bullet in &mut token_revolver.bullets {
            bullet.created_at = created_at;
        }
        true
    }

    fn apply_recommended_action(&mut self, mint: &Pubkey, action: ManagedPositionAction) -> bool {
        let Some(token_revolver) = self.tokens.get_mut(mint) else {
            return false;
        };

        match action {
            ManagedPositionAction::TightenStop => {
                if !token_revolver.is_panic_sell() {
                    token_revolver.set_tight_stop_loss();
                }
            }
            ManagedPositionAction::PanicSell => {
                token_revolver.set_panic_sell();
            }
        }
        true
    }

    pub fn register_position_epoch(&mut self, position_id: &str, position_epoch: u64) {
        let Some(mint) = self.position_mints.get(position_id).copied() else {
            return;
        };
        if let Some(token_revolver) = self.tokens.get_mut(&mint) {
            token_revolver.register_position_epoch(position_id.to_string(), position_epoch);
        }
    }

    pub fn unregister_position_epoch(&mut self, position_id: &str) {
        let Some(mint) = self.position_mints.get(position_id).copied() else {
            return;
        };
        if let Some(token_revolver) = self.tokens.get_mut(&mint) {
            token_revolver.unregister_position_epoch();
        }
    }

    pub fn get_execution_stress_by_position(
        &self,
        position_id: &str,
    ) -> Option<trigger::ExecutionStressSnapshot> {
        let mint = self.position_mints.get(position_id)?;
        let token = self.tokens.get(mint)?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        Some(token.get_execution_stress(now_ms))
    }

    pub fn process_market_snapshot(
        &mut self,
        base_mint: &Pubkey,
        current_price: f64,
        now_ms: u64,
    ) -> Vec<ShadowExitExecution> {
        let Some(current_price_key) = scale_price_to_target_key(current_price) else {
            return Vec::new();
        };

        let mut should_remove = None::<String>;
        let mut exits = Vec::new();

        if let Some(token_revolver) = self.tokens.get_mut(base_mint) {
            let bullet_indices = token_revolver.check_targets(current_price_key);
            if bullet_indices.is_empty() {
                return exits;
            }

            let Some(position_id) = token_revolver.position_id.clone() else {
                return exits;
            };
            let position_epoch = token_revolver.position_epoch;
            token_revolver.record_sell_attempt(now_ms);
            let bullets = token_revolver.take_bullets(&bullet_indices);

            let mut remaining_fraction_bps: u16 = bullets
                .iter()
                .map(|bullet| bullet.position_fraction_bps as u32)
                .sum::<u32>()
                .saturating_add(
                    token_revolver
                        .bullets
                        .iter()
                        .map(|bullet| bullet.position_fraction_bps as u32)
                        .sum::<u32>(),
                )
                .min(10_000) as u16;

            for bullet in bullets {
                remaining_fraction_bps =
                    remaining_fraction_bps.saturating_sub(bullet.position_fraction_bps);
                exits.push(ShadowExitExecution {
                    position_id: position_id.clone(),
                    position_epoch,
                    fraction_bps: bullet.position_fraction_bps,
                    remaining_fraction_bps,
                    fill_price: current_price,
                });
            }

            if token_revolver.is_empty() {
                should_remove = Some(position_id);
            }
        }

        if let Some(position_id) = should_remove {
            self.remove_position(&position_id);
        }

        exits
    }
}

#[derive(Debug, Clone, Default)]
pub struct PositionRuntimeRouter {
    live_revolver: Option<Arc<RwLock<Revolver>>>,
    shadow_book: Option<Arc<RwLock<ShadowPositionBook>>>,
}

impl PositionRuntimeRouter {
    pub fn with_live_revolver(revolver: Arc<RwLock<Revolver>>) -> Self {
        Self {
            live_revolver: Some(revolver),
            shadow_book: None,
        }
    }

    pub fn with_shadow_book(shadow_book: Arc<RwLock<ShadowPositionBook>>) -> Self {
        Self {
            live_revolver: None,
            shadow_book: Some(shadow_book),
        }
    }

    pub fn live_revolver(&self) -> Option<Arc<RwLock<Revolver>>> {
        self.live_revolver.clone()
    }

    pub fn shadow_book(&self) -> Option<Arc<RwLock<ShadowPositionBook>>> {
        self.shadow_book.clone()
    }

    fn uses_live_sink(lane: Lane) -> bool {
        matches!(lane, Lane::Live | Lane::Paper | Lane::Single)
    }

    async fn apply_recommended_action(
        &self,
        signal: &GuardianSignal,
        action: ManagedPositionAction,
    ) -> bool {
        if Self::uses_live_sink(signal.lane) {
            let Some(revolver) = self.live_revolver.as_ref() else {
                return false;
            };
            let mut revolver = revolver.write().await;
            let Some(token_revolver) = revolver.get_revolver_mut(&signal.base_mint) else {
                return false;
            };
            match action {
                ManagedPositionAction::TightenStop => {
                    if !token_revolver.is_panic_sell() {
                        token_revolver.set_tight_stop_loss();
                    }
                }
                ManagedPositionAction::PanicSell => token_revolver.set_panic_sell(),
            }
            return true;
        }

        let Some(shadow_book) = self.shadow_book.as_ref() else {
            return false;
        };
        shadow_book
            .write()
            .await
            .apply_recommended_action(&signal.base_mint, action)
    }

    pub async fn is_position_active(
        &self,
        lane: Lane,
        base_mint: &Pubkey,
        position_id: &str,
    ) -> bool {
        if Self::uses_live_sink(lane) {
            let Some(revolver) = self.live_revolver.as_ref() else {
                return false;
            };
            let revolver = revolver.read().await;
            return revolver
                .get_revolver(base_mint)
                .is_some_and(|token_revolver| {
                    token_revolver.total_artifact_count() > 0
                        && token_revolver
                            .position_id
                            .as_deref()
                            .map(|id| id == position_id)
                            .unwrap_or(true)
                });
        }

        let Some(shadow_book) = self.shadow_book.as_ref() else {
            return false;
        };
        shadow_book
            .read()
            .await
            .is_position_active(base_mint, position_id)
    }
}

#[derive(Debug)]
pub struct ShadowPositionBookAemAdapter<'a> {
    shadow_book: &'a mut ShadowPositionBook,
}

impl<'a> ShadowPositionBookAemAdapter<'a> {
    pub fn new(shadow_book: &'a mut ShadowPositionBook) -> Self {
        Self { shadow_book }
    }
}

impl<'a> TriggerControlAdapter for ShadowPositionBookAemAdapter<'a> {
    fn apply_control_command(
        &mut self,
        cmd: &ControlCommand,
        now_unix_ms: u64,
    ) -> CommandApplyResult {
        let Some(mint) = self
            .shadow_book
            .position_mints
            .get(&cmd.position_id)
            .copied()
        else {
            return CommandApplyResult {
                accepted: false,
                reject_reason: Some("position_not_found".to_string()),
            };
        };
        let Some(token_revolver) = self.shadow_book.tokens.get_mut(&mint) else {
            return CommandApplyResult {
                accepted: false,
                reject_reason: Some("position_not_found".to_string()),
            };
        };

        let priority = match cmd.priority {
            CommandPriority::Default => trigger::AemCommandPriority::Default,
            CommandPriority::AemPolicy => trigger::AemCommandPriority::AemPolicy,
            CommandPriority::HardSafety => trigger::AemCommandPriority::HardSafety,
        };
        let directive = match cmd.directive {
            CommandDirective::Noop => trigger::AemCommandDirective::Noop,
            CommandDirective::SetTightStop => trigger::AemCommandDirective::SetTightStop,
            CommandDirective::SetLooseStop => trigger::AemCommandDirective::SetLooseStop,
            CommandDirective::ForceExitAll => trigger::AemCommandDirective::ForceExitAll,
            CommandDirective::ForceExitFractionBps { fraction_bps } => {
                trigger::AemCommandDirective::ForceExitFractionBps { fraction_bps }
            }
            CommandDirective::FreezePanic => trigger::AemCommandDirective::FreezePanic,
        };
        let out = token_revolver.apply_aem_control_command(
            cmd.issued_at_unix_ms,
            cmd.valid_from_unix_ms,
            cmd.expires_at_unix_ms,
            cmd.position_epoch,
            priority,
            directive,
            &cmd.reason_code,
            now_unix_ms,
        );
        CommandApplyResult {
            accepted: out.accepted,
            reject_reason: out.reject_reason,
        }
    }

    fn get_execution_stress(&self, position_id: &str) -> Option<ExecutionStressSnapshot> {
        self.shadow_book
            .get_execution_stress_by_position(position_id)
            .map(|snapshot| ExecutionStressSnapshot {
                requeue_count: snapshot.requeue_count,
                send_fail_count: snapshot.send_fail_count,
                relax_count: snapshot.relax_count,
                oracle_stale_age_ms: snapshot.oracle_stale_age_ms,
                last_sell_attempt_age_ms: snapshot.last_sell_attempt_age_ms,
            })
    }

    fn register_position_epoch(&mut self, position_id: &str, position_epoch: u64) {
        self.shadow_book
            .register_position_epoch(position_id, position_epoch);
    }

    fn unregister_position_epoch(&mut self, position_id: &str) {
        self.shadow_book.unregister_position_epoch(position_id);
    }
}

/// Routes `GuardianSignal` events to the appropriate managed-position sink.
pub struct SignalRouter {
    signal_rx: mpsc::Receiver<GuardianSignal>,
    runtime_router: Arc<PositionRuntimeRouter>,
    signals_processed: u64,
    action_counts: ActionCounts,
}

#[derive(Debug, Default)]
struct ActionCounts {
    hold: u64,
    tighten_stop: u64,
    defensive_mode: u64,
    panic_sell: u64,
    unknown_mint: u64,
}

impl SignalRouter {
    pub fn new(
        signal_rx: mpsc::Receiver<GuardianSignal>,
        runtime_router: Arc<PositionRuntimeRouter>,
    ) -> Self {
        Self {
            signal_rx,
            runtime_router,
            signals_processed: 0,
            action_counts: ActionCounts::default(),
        }
    }

    pub async fn run(mut self) {
        info!("🛡️ SignalRouter: Started — awaiting guardian signals");

        while let Some(signal) = self.signal_rx.recv().await {
            self.signals_processed += 1;
            self.route_signal(&signal).await;

            if self.signals_processed % 100 == 0 {
                info!(
                    "🛡️ SignalRouter: Processed {} signals — hold={} tighten={} defensive={} panic={} unknown_mint={}",
                    self.signals_processed,
                    self.action_counts.hold,
                    self.action_counts.tighten_stop,
                    self.action_counts.defensive_mode,
                    self.action_counts.panic_sell,
                    self.action_counts.unknown_mint,
                );
            }
        }

        info!(
            "🛡️ SignalRouter: Channel closed — total signals processed: {} (hold={} tighten={} defensive={} panic={})",
            self.signals_processed,
            self.action_counts.hold,
            self.action_counts.tighten_stop,
            self.action_counts.defensive_mode,
            self.action_counts.panic_sell,
        );
    }

    async fn route_signal(&mut self, signal: &GuardianSignal) {
        let action = self.determine_action(signal);

        match action {
            RecommendedAction::Hold => {
                self.action_counts.hold += 1;
                debug!(
                    "🛡️ SignalRouter: HOLD — lane={} mint={} source={:?} severity={:?} reason={}",
                    signal.lane, signal.base_mint, signal.source, signal.severity, signal.reason
                );
            }
            RecommendedAction::TightenStop => {
                self.action_counts.tighten_stop += 1;
                self.apply_position_strategy(signal, ManagedPositionAction::TightenStop)
                    .await;
            }
            RecommendedAction::DefensiveMode => {
                self.action_counts.defensive_mode += 1;
                warn!(
                    "🛡️ SignalRouter: DEFENSIVE MODE — lane={} mint={} source={:?} reason={}",
                    signal.lane, signal.base_mint, signal.source, signal.reason
                );
                self.apply_position_strategy(signal, ManagedPositionAction::TightenStop)
                    .await;
            }
            RecommendedAction::PanicSell => {
                self.action_counts.panic_sell += 1;
                error!(
                    "🛡️ SignalRouter: PANIC SELL — lane={} mint={} source={:?} reason={}",
                    signal.lane, signal.base_mint, signal.source, signal.reason
                );
                self.apply_position_strategy(signal, ManagedPositionAction::PanicSell)
                    .await;
            }
        }
    }

    fn determine_action(&self, signal: &GuardianSignal) -> RecommendedAction {
        match signal.severity {
            SignalSeverity::Critical => RecommendedAction::PanicSell,
            SignalSeverity::Warning => {
                use super::signals::SignalSource;
                if signal.source == SignalSource::Whf {
                    RecommendedAction::DefensiveMode
                } else {
                    RecommendedAction::TightenStop
                }
            }
            SignalSeverity::Info => RecommendedAction::Hold,
        }
    }

    async fn apply_position_strategy(
        &mut self,
        signal: &GuardianSignal,
        action: ManagedPositionAction,
    ) {
        if !self
            .runtime_router
            .apply_recommended_action(signal, action)
            .await
        {
            self.action_counts.unknown_mint += 1;
            warn!(
                "🛡️ SignalRouter: No managed position sink for lane={} mint={} — signal discarded (source={:?})",
                signal.lane, signal.base_mint, signal.source
            );
            return;
        }

        match action {
            ManagedPositionAction::TightenStop => info!(
                "🛡️ SignalRouter: Strategy → TightStopLoss for lane={} mint={} (source={:?}, confidence={:.2})",
                signal.lane, signal.base_mint, signal.source, signal.confidence
            ),
            ManagedPositionAction::PanicSell => error!(
                "🛡️ SignalRouter: Strategy → PanicSell for lane={} mint={} (source={:?}, confidence={:.2})",
                signal.lane, signal.base_mint, signal.source, signal.confidence
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::signals::{SignalSeverity, SignalSource};
    use super::*;

    fn make_signal(source: SignalSource, severity: SignalSeverity, lane: Lane) -> GuardianSignal {
        GuardianSignal {
            lane,
            position_id: None,
            base_mint: Pubkey::new_unique(),
            pool_amm_id: Pubkey::new_unique(),
            source,
            severity,
            reason: "test".to_string(),
            confidence: 0.9,
            timestamp_ms: 1000,
            raw_score: None,
        }
    }

    #[test]
    fn determine_action_critical_is_panic() {
        let revolver = Arc::new(RwLock::new(Revolver::new()));
        let (_tx, rx) = mpsc::channel(16);
        let router = SignalRouter::new(
            rx,
            Arc::new(PositionRuntimeRouter::with_live_revolver(revolver)),
        );

        let signal = make_signal(SignalSource::Ligma, SignalSeverity::Critical, Lane::Live);
        assert_eq!(
            router.determine_action(&signal),
            RecommendedAction::PanicSell
        );
    }

    #[test]
    fn determine_action_whf_warning_is_defensive() {
        let revolver = Arc::new(RwLock::new(Revolver::new()));
        let (_tx, rx) = mpsc::channel(16);
        let router = SignalRouter::new(
            rx,
            Arc::new(PositionRuntimeRouter::with_live_revolver(revolver)),
        );

        let signal = make_signal(SignalSource::Whf, SignalSeverity::Warning, Lane::Live);
        assert_eq!(
            router.determine_action(&signal),
            RecommendedAction::DefensiveMode
        );
    }

    #[test]
    fn determine_action_tcf_warning_is_tighten() {
        let revolver = Arc::new(RwLock::new(Revolver::new()));
        let (_tx, rx) = mpsc::channel(16);
        let router = SignalRouter::new(
            rx,
            Arc::new(PositionRuntimeRouter::with_live_revolver(revolver)),
        );

        let signal = make_signal(SignalSource::Tcf, SignalSeverity::Warning, Lane::Live);
        assert_eq!(
            router.determine_action(&signal),
            RecommendedAction::TightenStop
        );
    }

    #[test]
    fn determine_action_info_is_hold() {
        let revolver = Arc::new(RwLock::new(Revolver::new()));
        let (_tx, rx) = mpsc::channel(16);
        let router = SignalRouter::new(
            rx,
            Arc::new(PositionRuntimeRouter::with_live_revolver(revolver)),
        );

        let signal = make_signal(SignalSource::Panic, SignalSeverity::Info, Lane::Live);
        assert_eq!(router.determine_action(&signal), RecommendedAction::Hold);
    }

    #[test]
    fn shadow_position_book_registers_virtual_magazine_and_triggers_targets() {
        let mint = Pubkey::new_unique();
        let mut book = ShadowPositionBook::new();
        book.register_position(mint, "shadow:mint:1", 7, 1.0)
            .expect("register shadow position");

        let token = book.tokens.get(&mint).expect("shadow token");
        assert_eq!(token.bullet_count(), 3);
        assert_eq!(
            token.bullets[0].target_price,
            (2.0 * SHADOW_PRICE_SCALE) as u64
        );
        assert!(token
            .bullets
            .iter()
            .all(|bullet| bullet.tx_bytes.is_empty()));

        let exits = book.process_market_snapshot(&mint, 2.0, 1_000);
        assert_eq!(exits.len(), 1);
        assert_eq!(exits[0].fraction_bps, 2_500);
        assert_eq!(exits[0].remaining_fraction_bps, 7_500);
    }

    #[test]
    fn shadow_position_book_can_peek_triggered_fraction_without_consuming_bullets() {
        let mint = Pubkey::new_unique();
        let mut book = ShadowPositionBook::new();
        book.register_position(mint, "shadow:mint:peek", 1, 1.0)
            .expect("register shadow position");

        let preview = book.peek_exit_fraction_bps(&mint, 10.0);
        assert_eq!(preview, 10_000);
        assert_eq!(
            book.tokens.get(&mint).expect("shadow token").bullet_count(),
            3
        );
    }

    #[test]
    fn shadow_position_book_preserves_ultra_low_entry_prices() {
        let mint = Pubkey::new_unique();
        let mut book = ShadowPositionBook::new();
        let entry_price = 0.00000001_f64;
        book.register_position(mint, "shadow:mint:micro", 1, entry_price)
            .expect("register shadow position with micro price");

        let token = book.tokens.get(&mint).expect("shadow token");
        assert_eq!(token.bullet_count(), 3);
        assert!(token.bullets[0].target_price > 0);
        assert_eq!(
            token
                .bullets
                .iter()
                .map(|bullet| bullet.time_stop_secs)
                .collect::<Vec<_>>(),
            vec![
                Some(SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS),
                Some(SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS),
                Some(SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS),
            ]
        );

        let preview = book.peek_exit_fraction_bps(&mint, entry_price * 2.0);
        assert_eq!(preview, 2_500);
    }

    #[test]
    fn shadow_position_book_time_stop_can_trigger_full_exit() {
        let mint = Pubkey::new_unique();
        let mut book = ShadowPositionBook::new();
        book.register_position(mint, "shadow:mint:timeout", 1, 1.0)
            .expect("register shadow position");
        assert!(book.age_position_for_time_stop_for_tests(
            &mint,
            SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS + 1
        ));

        let exits = book.process_market_snapshot(&mint, 1.0, 301_000);
        assert_eq!(exits.len(), 3);
        assert_eq!(
            exits
                .iter()
                .map(|exit| exit.fraction_bps as u32)
                .sum::<u32>(),
            10_000
        );
        assert!(!book.has_position("shadow:mint:timeout"));
    }

    #[test]
    fn shadow_position_book_preview_marks_timeout_trigger_below_target() {
        let mint = Pubkey::new_unique();
        let mut book = ShadowPositionBook::new();
        book.register_position(mint, "shadow:mint:timeout-preview", 1, 1.0)
            .expect("register shadow position");
        assert!(book.age_position_for_time_stop_for_tests(
            &mint,
            SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS + 1
        ));

        let preview = book.preview_exit(&mint, 1.3);
        assert_eq!(preview.fraction_bps, 10_000);
        assert!(preview.has_time_stop_trigger);
        assert_eq!(
            book.tokens.get(&mint).expect("shadow token").bullet_count(),
            3
        );
    }

    #[test]
    fn shadow_position_book_refreshes_timeout_anchor_on_market_activity() {
        let mint = Pubkey::new_unique();
        let mut book = ShadowPositionBook::new();
        book.register_position(mint, "shadow:mint:timeout-refresh", 1, 1.0)
            .expect("register shadow position");
        assert!(book.age_position_for_time_stop_for_tests(
            &mint,
            SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS + 1
        ));

        let preview_before_refresh = book.preview_exit(&mint, 1.3);
        assert_eq!(preview_before_refresh.fraction_bps, 10_000);
        assert!(preview_before_refresh.has_time_stop_trigger);

        assert!(book.refresh_time_stop_anchor(&mint));
        let preview_after_refresh = book.preview_exit(&mint, 1.3);
        assert_eq!(preview_after_refresh.fraction_bps, 0);
        assert!(!preview_after_refresh.has_time_stop_trigger);
    }

    #[tokio::test]
    async fn router_processes_live_signal_end_to_end() {
        let revolver = Arc::new(RwLock::new(Revolver::new()));
        let (tx, rx) = mpsc::channel(16);

        let mint = Pubkey::new_unique();
        {
            let mut rev = revolver.write().await;
            rev.load_magazine(mint, vec![]);
        }

        let router = SignalRouter::new(
            rx,
            Arc::new(PositionRuntimeRouter::with_live_revolver(Arc::clone(
                &revolver,
            ))),
        );
        let handle = tokio::spawn(router.run());

        tx.send(GuardianSignal {
            lane: Lane::Live,
            position_id: None,
            base_mint: mint,
            pool_amm_id: Pubkey::new_unique(),
            source: SignalSource::Ligma,
            severity: SignalSeverity::Critical,
            reason: "Liquidity trap detected".to_string(),
            confidence: 0.95,
            timestamp_ms: 1234,
            raw_score: Some(9500.0),
        })
        .await
        .unwrap();
        drop(tx);
        handle.await.unwrap();

        let rev = revolver.read().await;
        let token_rev = rev.get_revolver(&mint).unwrap();
        assert!(token_rev.is_panic_sell());
    }

    #[tokio::test]
    async fn router_processes_shadow_signal_end_to_end() {
        let shadow_book = Arc::new(RwLock::new(ShadowPositionBook::new()));
        let (tx, rx) = mpsc::channel(16);

        let mint = Pubkey::new_unique();
        {
            let mut book = shadow_book.write().await;
            book.register_position(mint, "shadow:mint:1", 1, 1.0)
                .expect("register shadow position");
        }

        let router = SignalRouter::new(
            rx,
            Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::clone(
                &shadow_book,
            ))),
        );
        let handle = tokio::spawn(router.run());

        tx.send(GuardianSignal {
            lane: Lane::Shadow,
            position_id: Some("shadow:mint:1".to_string()),
            base_mint: mint,
            pool_amm_id: Pubkey::new_unique(),
            source: SignalSource::Ligma,
            severity: SignalSeverity::Critical,
            reason: "Shadow panic".to_string(),
            confidence: 0.98,
            timestamp_ms: 1234,
            raw_score: Some(9900.0),
        })
        .await
        .unwrap();
        drop(tx);
        handle.await.unwrap();

        let book = shadow_book.read().await;
        let token_rev = book.tokens.get(&mint).unwrap();
        assert!(token_rev.is_panic_sell());
    }
}
