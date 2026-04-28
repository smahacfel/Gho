//! Self-driving paper position lifecycle for ghost-brain.
//!
//! Legacy note: canonical mirrored shadow runtime no longer builds on this module.
//! PR-4 freezes shadow position management around Guardian + lane-aware sinks, so
//! `PaperPositionLifecycle` remains a legacy/test helper for `ExecutionMode::Paper`.
//!
//! Encapsulates the complete post-buy lifecycle for PAPER mode:
//! - Entry submission + fill polling via `PaperBroker`
//! - AEM tick loop + management decisions via `AemRuntime`
//! - Exit submission + fill polling via `PaperBroker`
//! - JSONL event output via `EventEmitter`
//!
//! The launcher never owns tick loop / exit logic — ghost-brain self-drives.
//!
//! ## Usage from launcher (thin adapter)
//!
//! ```ignore
//! let lifecycle = PaperPositionLifecycle::new(config, emitter, quote_provider);
//! lifecycle.run(candidate_ref, epoch).await;
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::aem::types::{
    ActionChosen, AemLedgerWriter, CommandApplyResult, CommandDirective, ControlCommand,
    ExecutionStressSnapshot, ManagementDecisionEvent, ManagementOutcomeEvent, ReclaimFlag,
    RegimeIndexRecord, StateFeatures, StressBucket, TimeIndexRecord, TriggerControlAdapter, UnixMs,
};
use crate::aem::{AemConfig, AemRuntime};
use crate::events::{CloseReason, EventEmitter};
use crate::execution::backend::{CandidateRef, OrderSide, PositionId};
use crate::execution::paper::{PaperBroker, PaperBrokerConfig};
use crate::quotes::provider::{ExecutableQuoteProvider, QuoteSource};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

// ─── Config ─────────────────────────────────────────────────────────────────

/// Configuration for a self-driving paper position lifecycle.
#[derive(Debug, Clone)]
pub struct PaperLifecycleConfig {
    /// Paper fill delay range (min ms).
    pub fill_delay_min_ms: u64,
    /// Paper fill delay range (max ms).
    pub fill_delay_max_ms: u64,
    /// AEM tick interval in ms.
    pub tick_interval_ms: u64,
    /// Number of ticks before automatic exit (safety net).
    pub max_ticks: u64,
    /// AEM outcome horizon in seconds (`AemConfig.t_s`).
    pub aem_t_s: u64,
    /// Maximum number of open paper positions allowed concurrently.
    pub max_open_positions: usize,
}

impl Default for PaperLifecycleConfig {
    fn default() -> Self {
        Self {
            fill_delay_min_ms: 200,
            fill_delay_max_ms: 400,
            tick_interval_ms: 500,
            max_ticks: 240,
            aem_t_s: 120,
            max_open_positions: 1,
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn token_units_to_tokens(fill_qty: u64) -> f64 {
    fill_qty as f64 / LAMPORTS_PER_SOL
}

fn position_value_sol(fill_price: f64, fill_qty: u64) -> f64 {
    fill_price * token_units_to_tokens(fill_qty)
}

fn synthetic_mark_price(candidate_id: &str, entry_price: f64, tick_count: u64) -> f64 {
    let seed = candidate_id.bytes().fold(0u64, |acc, byte| {
        acc.wrapping_mul(131).wrapping_add(byte as u64)
    });
    let direction = if seed & 1 == 0 { 1.0 } else { -1.0 };
    let drift_bps = 6.0 + (seed % 19) as f64;
    let oscillation_bps = 2.0 + ((seed >> 8) % 7) as f64;
    let phase = tick_count as f64 + (seed % 5) as f64;
    let drift = direction * drift_bps * tick_count as f64 / 10_000.0;
    let oscillation = phase.sin() * oscillation_bps / 10_000.0;
    (entry_price * (1.0 + drift + oscillation)).max(entry_price * 0.05)
}

/// Minimal AemLedgerWriter — events are persisted via EventEmitter, not the ledger.
struct NullLedger;

impl AemLedgerWriter for NullLedger {
    fn append_decision(
        &self,
        _event: &ManagementDecisionEvent,
    ) -> Result<(), crate::aem::AemError> {
        Ok(())
    }
    fn append_outcome(&self, _event: &ManagementOutcomeEvent) -> Result<(), crate::aem::AemError> {
        Ok(())
    }
    fn append_time_index(&self, _idx: &TimeIndexRecord) -> Result<(), crate::aem::AemError> {
        Ok(())
    }
    fn append_regime_index(&self, _idx: &RegimeIndexRecord) -> Result<(), crate::aem::AemError> {
        Ok(())
    }
}

/// TriggerControlAdapter that captures exit commands from AEM.
struct BridgeTriggerAdapter {
    pending_commands: Vec<ControlCommand>,
    stress_snapshots: HashMap<String, ExecutionStressSnapshot>,
}

impl BridgeTriggerAdapter {
    fn new() -> Self {
        Self {
            pending_commands: Vec::new(),
            stress_snapshots: HashMap::new(),
        }
    }

    fn take_pending_commands(&mut self) -> Vec<ControlCommand> {
        std::mem::take(&mut self.pending_commands)
    }
}

impl TriggerControlAdapter for BridgeTriggerAdapter {
    fn apply_control_command(
        &mut self,
        cmd: &ControlCommand,
        _now_unix_ms: UnixMs,
    ) -> CommandApplyResult {
        self.pending_commands.push(cmd.clone());
        CommandApplyResult {
            accepted: true,
            reject_reason: None,
        }
    }

    fn get_execution_stress(&self, position_id: &str) -> Option<ExecutionStressSnapshot> {
        self.stress_snapshots.get(position_id).cloned()
    }

    fn register_position_epoch(&mut self, _position_id: &str, _position_epoch: u64) {}
    fn unregister_position_epoch(&mut self, _position_id: &str) {}
}

// ─── Lifecycle runner ───────────────────────────────────────────────────────

/// Self-driving paper position lifecycle.
///
/// Owns the PaperBroker, AemRuntime, and QuoteProvider internally.
/// The caller provides an `EventEmitter` and `CandidateRef`, then calls `run()`.
/// All tick loop, fill polling, exit decisions, and event emission happen here
/// in ghost-brain — the launcher never owns any of this logic.
pub struct PaperPositionLifecycle {
    config: PaperLifecycleConfig,
    emitter: Arc<EventEmitter>,
    quote_provider: Arc<RwLock<ExecutableQuoteProvider>>,
    broker: Arc<RwLock<PaperBroker>>,
}

impl PaperPositionLifecycle {
    /// Create a new lifecycle runner.
    ///
    /// The `EventEmitter` and `QuoteProvider` are shared across positions.
    /// A `PaperBroker` is created internally from the config.
    pub fn new(
        config: PaperLifecycleConfig,
        emitter: Arc<EventEmitter>,
        quote_provider: Arc<RwLock<ExecutableQuoteProvider>>,
    ) -> Self {
        let broker_config = PaperBrokerConfig {
            fill_delay_ms_min: config.fill_delay_min_ms,
            fill_delay_ms_max: config.fill_delay_max_ms,
            max_open_positions_paper: config.max_open_positions.max(1),
            candidate_sampling: 1.0,
            rng_seed: 42,
            ..PaperBrokerConfig::default()
        };
        let broker = PaperBroker::new(broker_config, quote_provider.clone());
        Self {
            config,
            emitter,
            quote_provider,
            broker: Arc::new(RwLock::new(broker)),
        }
    }

    /// Run the full paper position lifecycle for one candidate.
    ///
    /// This is the ONLY entrypoint the launcher calls. Ghost-brain self-drives:
    /// entry → fill → AEM ticks → exit → close.
    pub async fn run(&self, candidate_ref: CandidateRef, epoch: u64, entry_price: f64) {
        let candidate_id = candidate_ref.candidate_id.clone();
        let pool_pubkey = candidate_ref.pool_amm_id;
        let mint_pubkey = candidate_ref.base_mint;
        let amount_lamports = candidate_ref.entry_amount_lamports;

        let tick_interval = tokio::time::Duration::from_millis(self.config.tick_interval_ms);

        let aem_config = AemConfig {
            enabled: true,
            t_s: self.config.aem_t_s,
            min_stabilization_ticks: 2,
            ..AemConfig::default()
        };
        let ledger = NullLedger;
        let mut trigger = BridgeTriggerAdapter::new();
        let mut aem = AemRuntime::new(aem_config);

        // 1. Emit Candidate
        self.emitter.emit_candidate(
            &candidate_id,
            Some(entry_price),
            Some(entry_price),
            "PASS",
            vec![],
            "launcher",
        );

        // 2. Seed a quote so PaperBroker can resolve fills
        {
            let mut qp = self.quote_provider.write().await;
            qp.generate_quote(
                &pool_pubkey,
                &mint_pubkey,
                now_ms(),
                Some(0),
                entry_price,
                amount_lamports,
                1_000_000,
                0.0,
                QuoteSource::External,
            );
        }

        // 3. Submit entry via PaperBroker
        let quote_id = {
            let qp = self.quote_provider.read().await;
            qp.latest_quote(&pool_pubkey)
                .map(|q| q.quote_id.clone())
                .unwrap_or_else(|| "q-0".to_string())
        };

        let order_id = {
            let mut broker = self.broker.write().await;
            match broker.submit_entry(&candidate_ref, quote_id.clone(), now_ms()) {
                Ok(oid) => {
                    self.emitter.emit_entry_submitted(
                        &candidate_id,
                        &oid,
                        amount_lamports,
                        1,
                        None,
                    );
                    info!(
                        candidate_id = %candidate_id,
                        order_id = %oid,
                        "PaperLifecycle: entry submitted"
                    );
                    oid
                }
                Err(e) => {
                    warn!("PaperLifecycle: submit_entry failed: {:?}", e);
                    return;
                }
            }
        };

        // 4. Poll for entry fill
        let mut position_id: Option<PositionId> = None;
        let mut entry_fill_price = entry_price;
        let mut entry_fill_time_ms = now_ms();
        let mut entry_fill_qty: u64 = 0;
        let poll_deadline = now_ms() + 5000;

        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            let fill = {
                let mut broker = self.broker.write().await;
                broker.take_fill_for_order(&order_id, now_ms()).await
            };
            if let Some(fill) = fill {
                if fill.side == OrderSide::Entry {
                    entry_fill_price = fill.fill_price;
                    entry_fill_time_ms = fill.fill_time_ms;
                    entry_fill_qty = fill.fill_qty;

                    self.emitter.emit_entry_filled(
                        &candidate_id,
                        &order_id,
                        fill.fill_time_ms,
                        fill.fill_price,
                        fill.fill_qty,
                        &fill.quote_id_used,
                        fill.status,
                        fill.latency_ms,
                    );

                    if let Some(ref pid) = fill.position_id {
                        position_id = Some(pid.clone());

                        self.emitter.emit_position_opened(
                            &candidate_id,
                            pid,
                            fill.fill_price,
                            fill.fill_time_ms,
                            epoch,
                            fill.fill_qty,
                            amount_lamports,
                        );

                        if let Err(e) = aem.register_position(
                            pid.clone(),
                            pool_pubkey,
                            mint_pubkey,
                            fill.fill_time_ms,
                            fill.fill_price,
                            epoch,
                        ) {
                            warn!(
                                candidate_id = %candidate_id,
                                position_id = %pid,
                                "PaperLifecycle: AEM register_position failed: {:?}",
                                e
                            );
                        }

                        info!(
                            candidate_id = %candidate_id,
                            position_id = %pid,
                            fill_price = fill.fill_price,
                            "PaperLifecycle: position opened"
                        );
                    }
                }
            }
            if position_id.is_some() || now_ms() > poll_deadline {
                break;
            }
        }

        let position_id = match position_id {
            Some(pid) => pid,
            None => {
                warn!(
                    candidate_id = %candidate_id,
                    order_id = %order_id,
                    "PaperLifecycle: entry fill never arrived, aborting"
                );
                return;
            }
        };

        // 5. AEM tick loop — ghost-brain self-drives decisions
        let entry_time_ms = entry_fill_time_ms;
        let mut tick_count: u64 = 0;
        let mut should_exit = false;
        let mut close_reason = CloseReason::Default;
        let mut current_mark_price = entry_fill_price;
        let mut last_mark_price = entry_fill_price;
        let mut peak_mark_price = entry_fill_price;
        let mut last_mark_time_ms = entry_time_ms;

        loop {
            tokio::time::sleep(tick_interval).await;
            tick_count += 1;

            let current_time = now_ms();
            let time_since_entry_s = ((current_time - entry_time_ms) / 1000) as u32;
            current_mark_price = synthetic_mark_price(&candidate_id, entry_fill_price, tick_count);
            peak_mark_price = peak_mark_price.max(current_mark_price);
            let elapsed_since_last_mark_s =
                ((current_time.saturating_sub(last_mark_time_ms)).max(1) as f64) / 1000.0;
            let drawdown_pct = if peak_mark_price > 0.0 {
                ((peak_mark_price - current_mark_price) / peak_mark_price) * 100.0
            } else {
                0.0
            };
            let unrealized_pnl_pct = if entry_fill_price > 0.0 {
                ((current_mark_price - entry_fill_price) / entry_fill_price) * 100.0
            } else {
                0.0
            };
            let slope_pct_per_s = if last_mark_price > 0.0 {
                ((current_mark_price - last_mark_price) / last_mark_price) * 100.0
                    / elapsed_since_last_mark_s
            } else {
                0.0
            };

            // Refresh quote using a deterministic synthetic mark path.
            {
                let mut qp = self.quote_provider.write().await;
                qp.generate_quote(
                    &pool_pubkey,
                    &mint_pubkey,
                    current_time,
                    Some(0),
                    current_mark_price,
                    amount_lamports,
                    1_000_000,
                    0.0,
                    QuoteSource::External,
                );
            }

            let features = StateFeatures {
                position_id: position_id.clone(),
                pool_amm_id: pool_pubkey,
                base_mint: mint_pubkey,
                entry_price_or_mcap: entry_fill_price,
                current_price_or_mcap: current_mark_price,
                peak_since_entry: peak_mark_price,
                drawdown_pct,
                unrealized_pnl_pct,
                slope_pct_per_s,
                volatility_proxy: None,
                reclaim_flag: ReclaimFlag::None,
                time_since_entry_s,
                time_since_last_peak_s: time_since_entry_s,
                requeue_count: 0,
                send_fail_count: 0,
                relax_count: 0,
                oracle_stale_age_ms: 0,
                last_sell_attempt_age_ms: None,
                stress_bucket: StressBucket::Low,
            };

            match aem.on_tick_with_report(
                features,
                current_time,
                &ledger,
                &mut trigger,
                Some(&self.emitter),
                Some(&candidate_id),
            ) {
                Ok(Some(report)) => {
                    debug!(
                        position_id = %position_id,
                        tick = tick_count,
                        action = ?report.decision.action_chosen,
                        "PaperLifecycle: AEM tick with decision"
                    );
                    match report.decision.action_chosen {
                        ActionChosen::SellNow | ActionChosen::Panic => {
                            should_exit = true;
                            close_reason = match report.decision.action_chosen {
                                ActionChosen::SellNow => CloseReason::Target,
                                ActionChosen::Panic => CloseReason::Panic,
                                _ => CloseReason::Default,
                            };
                        }
                        _ => {}
                    }
                }
                Ok(None) => {
                    debug!(
                        position_id = %position_id,
                        tick = tick_count,
                        "PaperLifecycle: AEM tick (no report)"
                    );
                }
                Err(e) => {
                    warn!("PaperLifecycle: AEM on_tick error: {:?}", e);
                }
            }

            last_mark_price = current_mark_price;
            last_mark_time_ms = current_time;

            // Process pending exit commands from AEM
            let pending = trigger.take_pending_commands();
            for cmd in &pending {
                if matches!(
                    cmd.directive,
                    CommandDirective::ForceExitAll | CommandDirective::ForceExitFractionBps { .. }
                ) {
                    should_exit = true;
                    close_reason = match cmd.action {
                        ActionChosen::Panic => CloseReason::Panic,
                        ActionChosen::SellNow | ActionChosen::Partial => CloseReason::Target,
                        ActionChosen::WaitReclaim => CloseReason::Manual,
                    };
                }
            }

            // Safety net: force exit after max ticks
            if tick_count >= self.config.max_ticks {
                should_exit = true;
                if matches!(close_reason, CloseReason::Default) {
                    close_reason = CloseReason::TimeStop;
                }
            }

            if should_exit {
                break;
            }
        }

        // 6. Exit via PaperBroker
        let exit_quote_id = {
            let qp = self.quote_provider.read().await;
            qp.latest_quote(&pool_pubkey)
                .map(|q| q.quote_id.clone())
                .unwrap_or_else(|| "q-exit".to_string())
        };

        let exit_order_id = {
            let mut broker = self.broker.write().await;
            match broker.submit_exit(&position_id, 10_000, exit_quote_id, None, now_ms()) {
                Ok(oid) => {
                    self.emitter.emit_exit_submitted(
                        &candidate_id,
                        &position_id,
                        &oid,
                        10_000,
                        None,
                    );
                    oid
                }
                Err(e) => {
                    warn!("PaperLifecycle: submit_exit failed: {:?}", e);
                    return;
                }
            }
        };

        // 7. Poll for exit fill
        let exit_deadline = now_ms() + 5000;
        let mut exit_filled = false;

        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            let fill = {
                let mut broker = self.broker.write().await;
                broker.take_fill_for_order(&exit_order_id, now_ms()).await
            };
            if let Some(fill) = fill {
                if fill.side == OrderSide::Exit {
                    let entry_value_sol = position_value_sol(entry_fill_price, entry_fill_qty);
                    let exit_value_sol = position_value_sol(fill.fill_price, fill.fill_qty);
                    let estimated_costs_sol = 0.0;
                    let gross_pnl_sol = exit_value_sol - entry_value_sol;
                    let net_pnl_sol = gross_pnl_sol - estimated_costs_sol;
                    let final_pnl_pct = if entry_value_sol > 0.0 {
                        (gross_pnl_sol / entry_value_sol) * 100.0
                    } else {
                        0.0
                    };

                    self.emitter.emit_exit_filled(
                        &candidate_id,
                        &position_id,
                        &exit_order_id,
                        fill.fill_price,
                        fill.fill_qty,
                        gross_pnl_sol,
                        fill.status,
                        false,
                        0,
                    );

                    let duration_ms = fill.fill_time_ms.saturating_sub(entry_time_ms);
                    self.emitter.emit_position_closed(
                        &candidate_id,
                        &position_id,
                        gross_pnl_sol,
                        final_pnl_pct,
                        Some(entry_value_sol),
                        Some(exit_value_sol),
                        Some(gross_pnl_sol),
                        Some(net_pnl_sol),
                        Some(estimated_costs_sol),
                        duration_ms,
                        close_reason,
                        1,
                    );

                    self.emitter.emit_management_outcome(
                        &candidate_id,
                        &position_id,
                        serde_json::json!({
                            "reason": "Completed",
                            "ticks": tick_count,
                            "entry_value_sol": entry_value_sol,
                            "exit_value_sol": exit_value_sol,
                            "gross_pnl_sol": gross_pnl_sol,
                            "net_pnl_sol": net_pnl_sol,
                            "estimated_costs_sol": estimated_costs_sol,
                            "close_reason": format!("{:?}", close_reason),
                        }),
                        None,
                    );

                    exit_filled = true;
                }
            }
            if exit_filled || now_ms() > exit_deadline {
                break;
            }
        }

        let _ = aem.unregister_position(&position_id);

        if let Err(e) = self.emitter.flush() {
            warn!("PaperLifecycle: flush error: {}", e);
        }

        info!(
            position_id = %position_id,
            ticks = tick_count,
            "PaperLifecycle: position lifecycle complete"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventWriterConfig;
    use crate::events::{EventKind, ExecutionEvent};
    use crate::execution::backend::Lane;
    use crate::quotes::provider::QuoteProviderConfig;
    use solana_sdk::pubkey::Pubkey;

    #[tokio::test]
    async fn test_paper_lifecycle_runs_to_completion() {
        let tmp_dir = tempfile::tempdir().expect("temp dir");
        let events_dir = tmp_dir.path().join("events");
        std::fs::create_dir_all(&events_dir).expect("create events dir");

        let writer_config = EventWriterConfig {
            output_dir: events_dir.to_string_lossy().to_string(),
            enable_aem_ticks: true,
            enable_optional_events: true,
            flush_interval_ms: 100,
            ..EventWriterConfig::default()
        };
        let emitter = Arc::new(
            EventEmitter::new(writer_config, "test-run".to_string(), Lane::Paper).unwrap(),
        );

        let qp = Arc::new(RwLock::new(ExecutableQuoteProvider::new(
            QuoteProviderConfig {
                max_quote_age_ms: 5000,
                ring_buffer_size: 256,
                generation_interval_ms: 100,
                stale_warning_threshold_ms: 3000,
            },
        )));

        let config = PaperLifecycleConfig {
            fill_delay_min_ms: 50,
            fill_delay_max_ms: 100,
            tick_interval_ms: 50,
            max_ticks: 5,
            aem_t_s: 1,
            max_open_positions: 1,
        };

        let lifecycle = PaperPositionLifecycle::new(config, emitter.clone(), qp);

        let candidate = CandidateRef {
            candidate_id: "test-mint_test-pool_0".to_string(),
            base_mint: Pubkey::new_unique(),
            pool_amm_id: Pubkey::new_unique(),
            entry_amount_lamports: 500_000_000,
            min_tokens_out: 1,
        };

        lifecycle.run(candidate, 1, 0.5).await;

        // Verify JSONL events were written
        let mut event_types = Vec::new();
        let mut position_closed_payload: Option<serde_json::Value> = None;
        if let Ok(entries) = std::fs::read_dir(&events_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "jsonl") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for line in content.lines() {
                            if let Ok(event) = serde_json::from_str::<ExecutionEvent>(line) {
                                let type_name = event.kind.type_name().to_string();
                                if let EventKind::PositionClosed(payload) = event.kind {
                                    position_closed_payload = Some(
                                        serde_json::to_value(payload).expect("serialize payload"),
                                    );
                                }
                                event_types.push(type_name);
                            }
                        }
                    }
                }
            }
        }

        assert!(!event_types.is_empty(), "Should have events");
        assert!(event_types.contains(&"Candidate".to_string()));
        assert!(event_types.contains(&"EntrySubmitted".to_string()));
        assert!(event_types.contains(&"PositionOpened".to_string()));
        assert!(event_types.contains(&"ExitSubmitted".to_string()));
        assert!(event_types.contains(&"PositionClosed".to_string()));

        let payload = position_closed_payload.expect("missing PositionClosed payload");
        for field in [
            "entry_value_sol",
            "exit_value_sol",
            "gross_pnl_sol",
            "net_pnl_sol",
            "estimated_costs_sol",
        ] {
            assert!(
                payload
                    .get(field)
                    .and_then(|value| value.as_f64())
                    .is_some(),
                "PositionClosed payload missing {field}: {payload:?}"
            );
        }

        let entry_value_sol = payload["entry_value_sol"]
            .as_f64()
            .expect("entry_value_sol as f64");
        let exit_value_sol = payload["exit_value_sol"]
            .as_f64()
            .expect("exit_value_sol as f64");
        let gross_pnl_sol = payload["gross_pnl_sol"]
            .as_f64()
            .expect("gross_pnl_sol as f64");

        assert!(
            (entry_value_sol - 0.5).abs() < 0.05,
            "entry_value_sol should stay near funded trade value, got {entry_value_sol}"
        );
        assert!(exit_value_sol.is_finite() && exit_value_sol > 0.0);
        assert!(gross_pnl_sol.is_finite());
        assert!(
            (exit_value_sol - entry_value_sol).abs() > 1e-6,
            "synthetic mark path should produce non-flat exit economics"
        );
    }

    #[tokio::test]
    async fn test_concurrent_paper_lifecycles_do_not_steal_each_others_fills() {
        let tmp_dir = tempfile::tempdir().expect("temp dir");
        let events_dir = tmp_dir.path().join("events");
        std::fs::create_dir_all(&events_dir).expect("create events dir");

        let writer_config = EventWriterConfig {
            output_dir: events_dir.to_string_lossy().to_string(),
            enable_aem_ticks: true,
            enable_optional_events: true,
            flush_interval_ms: 50,
            ..EventWriterConfig::default()
        };
        let emitter = Arc::new(
            EventEmitter::new(
                writer_config,
                "test-run-concurrent".to_string(),
                Lane::Paper,
            )
            .unwrap(),
        );

        let qp = Arc::new(RwLock::new(ExecutableQuoteProvider::new(
            QuoteProviderConfig {
                max_quote_age_ms: 5000,
                ring_buffer_size: 256,
                generation_interval_ms: 100,
                stale_warning_threshold_ms: 3000,
            },
        )));

        let config = PaperLifecycleConfig {
            fill_delay_min_ms: 10,
            fill_delay_max_ms: 20,
            tick_interval_ms: 10,
            max_ticks: 2,
            aem_t_s: 1,
            max_open_positions: 2,
        };

        let lifecycle = Arc::new(PaperPositionLifecycle::new(config, emitter.clone(), qp));

        let candidate_a = CandidateRef {
            candidate_id: "test-mint-a_test-pool-a_1".to_string(),
            base_mint: Pubkey::new_unique(),
            pool_amm_id: Pubkey::new_unique(),
            entry_amount_lamports: 500_000_000,
            min_tokens_out: 1,
        };
        let candidate_b = CandidateRef {
            candidate_id: "test-mint-b_test-pool-b_2".to_string(),
            base_mint: Pubkey::new_unique(),
            pool_amm_id: Pubkey::new_unique(),
            entry_amount_lamports: 600_000_000,
            min_tokens_out: 1,
        };

        let lifecycle_a = lifecycle.clone();
        let lifecycle_b = lifecycle.clone();
        let handle_a = tokio::spawn(async move {
            lifecycle_a.run(candidate_a, 1, 0.5).await;
        });
        let handle_b = tokio::spawn(async move {
            lifecycle_b.run(candidate_b, 2, 0.6).await;
        });

        handle_a.await.expect("candidate A join");
        handle_b.await.expect("candidate B join");
        emitter.flush().expect("flush events");

        let mut seen_opened: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut seen_closed: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&events_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "jsonl") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for line in content.lines() {
                            if let Ok(event) = serde_json::from_str::<ExecutionEvent>(line) {
                                match event.kind {
                                    EventKind::PositionOpened(_) => {
                                        *seen_opened
                                            .entry(event.envelope.candidate_id.clone())
                                            .or_default() += 1;
                                    }
                                    EventKind::PositionClosed(_) => {
                                        *seen_closed
                                            .entry(event.envelope.candidate_id.clone())
                                            .or_default() += 1;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        assert_eq!(seen_opened.get("test-mint-a_test-pool-a_1"), Some(&1));
        assert_eq!(seen_opened.get("test-mint-b_test-pool-b_2"), Some(&1));
        assert_eq!(seen_closed.get("test-mint-a_test-pool-a_1"), Some(&1));
        assert_eq!(seen_closed.get("test-mint-b_test-pool-b_2"), Some(&1));
    }
}
