//! Gatekeeper Commit Loop - Periodic Checks and Commits
//!
//! This module provides a background task that periodically checks launcher-owned
//! commit windows and persists those that are ready to transition into the live path.

use crate::events::{EventBusSender, GhostEvent};
use crate::oracle_runtime::OracleRuntime;
use ghost_core::shadow_ledger::{LivePipeline, LiveTxEvent, ShadowLedger};
use metrics::increment_counter;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

#[derive(Clone, Debug)]
pub struct GatekeeperCommitLoopConfig {
    pub check_interval_ms: u64,
}

impl Default for GatekeeperCommitLoopConfig {
    fn default() -> Self {
        Self {
            check_interval_ms: 100,
        }
    }
}

pub async fn run(
    oracle_runtime: Arc<OracleRuntime>,
    live_pipeline: Arc<LivePipeline>,
    shadow_ledger: Arc<ShadowLedger>,
    event_bus_tx: Option<EventBusSender>,
    mut shutdown_rx: broadcast::Receiver<()>,
    config: GatekeeperCommitLoopConfig,
) {
    info!(
        "🔐 GatekeeperCommitLoop: Starting (check_interval={}ms)",
        config.check_interval_ms
    );

    let mut interval = tokio::time::interval(Duration::from_millis(config.check_interval_ms));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut total_commits = 0u64;
    let total_failures = 0u64;
    let mut last_log = std::time::Instant::now();

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                info!("🔐 GatekeeperCommitLoop: Shutdown signal received");
                info!("   Total commits: {}, Total failures: {}", total_commits, total_failures);
                break;
            }
            _ = interval.tick() => {
                let coordinator = oracle_runtime.commit_coordinator();
                let committed = coordinator.process_ready_commits(&shadow_ledger);

                if !committed.is_empty() {
                    total_commits += committed.len() as u64;
                    info!("✅ GatekeeperCommitLoop: Committed {} buffers", committed.len());

                    for _ in 0..committed.len() {
                        increment_counter!("gatekeeper_commit_success_total");
                    }

                    for committed_window in committed {
                        let pool_id = committed_window.pool_id;
                        let base_mint = committed_window.base_mint;
                        let commit_result = committed_window.commit_result;

                        if let Some(ref tx) = event_bus_tx {
                            if let Err(err) = tx.send(GhostEvent::gatekeeper_committed(
                                pool_id.to_string(),
                                base_mint.to_string(),
                                commit_result.committed_count,
                                commit_result.merged_pending_count,
                            )) {
                                warn!(
                                    "GatekeeperCommitLoop: Failed to emit GatekeeperCommitted for mint={} pool={}: {}",
                                    base_mint,
                                    pool_id,
                                    err
                                );
                            }
                        }

                        let Some(last_snapshot) = commit_result.last_snapshot.clone() else {
                            warn!(
                                "GatekeeperCommitLoop: CommitResult missing bootstrap snapshot for mint={} (skipping LivePipeline init)",
                                base_mint
                            );
                            increment_counter!("live_pipeline_init_missing_snapshot_total");
                            continue;
                        };

                        oracle_runtime.remember_committed_snapshot(base_mint, &last_snapshot);
                        if !live_pipeline.is_initialized(&base_mint) {
                            live_pipeline.init_for_mint(base_mint, &last_snapshot);
                            debug!(
                                "🔓 GatekeeperCommitLoop: Initialized LivePipeline for committed mint={}",
                                base_mint
                            );
                            increment_counter!("live_pipeline_init_success_total");
                        }
                        oracle_runtime.mark_pool_committed(pool_id);

                        if !commit_result.pending_live.is_empty() {
                            debug!(
                                "🔄 GatekeeperCommitLoop: Forwarding {} pending_live TXs for mint={} into LivePipeline",
                                commit_result.pending_live.len(),
                                base_mint
                            );
                        }

                        for tx in commit_result.pending_live {
                            let evt = match LiveTxEvent::new(
                                base_mint,
                                tx.tx_key.slot,
                                tx.tx_key.tx_index,
                                tx.tx_key.signature,
                                tx.tx_key.timestamp_ms,
                                tx.side,
                                tx.d_sol_lamports,
                                tx.d_tok_units,
                                tx.dev_buy,
                                tx.trader,
                            ) {
                                Ok(evt) => evt,
                                Err(err) => {
                                    warn!(
                                        "GatekeeperCommitLoop: Failed to convert pending_live TX to LiveTxEvent for mint={} (slot={:?}): {}",
                                        base_mint,
                                        tx.tx_key.slot,
                                        err
                                    );
                                    increment_counter!("gatekeeper_pending_live_forward_convert_fail_total");
                                    continue;
                                }
                            };

                            match live_pipeline.process_event(evt) {
                                Ok(()) => increment_counter!("gatekeeper_pending_live_forwarded_total"),
                                Err(err) => {
                                    warn!(
                                        "GatekeeperCommitLoop: LivePipeline rejected forwarded pending_live TX for mint={} (slot={:?}): {}",
                                        base_mint,
                                        tx.tx_key.slot,
                                        err
                                    );
                                    increment_counter!("gatekeeper_pending_live_forward_rejected_total");
                                }
                            }
                        }

                        if coordinator.finalize_committed(&base_mint) {
                            debug!(
                                "🧹 GatekeeperCommitLoop: Removed committed buffer for mint={}",
                                base_mint
                            );
                        }
                    }
                }

                if last_log.elapsed().as_secs() >= 60 {
                    let stats = coordinator.stats();
                    info!(
                        "🔐 GatekeeperCommitLoop: Stats - commits={}, active_buffers={}",
                        total_commits,
                        stats.active_buffers,
                    );
                    last_log = std::time::Instant::now();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::gatekeeper::RuntimeCommitPhase;
    use crate::oracle_runtime::{OracleRuntime, OracleRuntimeConfig};
    use ghost_brain::fast_pipeline::EnhancedCandidate;
    use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
    use ghost_core::shadow_ledger::{BufferedTx, LivePipelineConfig, TradeSide, TxKey};
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::Signature;

    fn build_runtime(
        shadow_ledger: Arc<ShadowLedger>,
        live_pipeline: Arc<LivePipeline>,
    ) -> Arc<OracleRuntime> {
        #[allow(deprecated)]
        Arc::new(OracleRuntime::new_with_config(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
            None,
            None,
            live_pipeline,
            OracleRuntimeConfig {
                runtime_shadowledger_snapshots_enabled: false,
                ..OracleRuntimeConfig::default()
            },
        ))
    }

    fn sample_buffered_tx(slot: u64, tx_index: u32, timestamp_ms: u64) -> BufferedTx {
        BufferedTx::new(
            TxKey::new(
                timestamp_ms,
                Some(slot),
                Some(tx_index),
                Some(Signature::new_unique()),
                0,
            )
            .unwrap(),
            TradeSide::Buy,
            1_000_000_000,
            1_000_000,
            false,
            None,
        )
        .unwrap()
    }

    fn register_pool(runtime: &OracleRuntime, pool_id: Pubkey, base_mint: Pubkey) {
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            slot: Some(42),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));
        runtime.mark_pool_approved(pool_id);
        runtime.approved_pools().insert(pool_id);
    }

    fn wait_for_commit_phase(
        coordinator: &crate::components::gatekeeper::LauncherCommitCoordinator,
        base_mint: &Pubkey,
        expected: RuntimeCommitPhase,
    ) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if coordinator.commit_phase(base_mint) == Some(expected) {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("timed out waiting for coordinator phase {expected:?}");
    }

    #[tokio::test]
    async fn test_commit_loop_starts_and_stops() {
        let pipeline = Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
        let ledger = Arc::new(ShadowLedger::new());
        let runtime = build_runtime(Arc::clone(&ledger), Arc::clone(&pipeline));
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let handle = tokio::spawn(async move {
            run(
                runtime,
                pipeline,
                ledger,
                None,
                shutdown_rx,
                GatekeeperCommitLoopConfig {
                    check_interval_ms: 50,
                },
            )
            .await;
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        handle.abort();
    }

    #[tokio::test]
    async fn runtime_gatekeeper_commits_without_core_gatekeeper() {
        let pipeline = Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
        let ledger = Arc::new(ShadowLedger::new());
        let runtime = build_runtime(Arc::clone(&ledger), Arc::clone(&pipeline));
        let (event_tx, mut event_rx) = crate::events::create_event_bus();
        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        register_pool(&runtime, pool_id, base_mint);
        runtime.commit_coordinator().stage_history(
            pool_id,
            base_mint,
            30_000_000_000,
            1_073_000_000_000_000,
            vec![sample_buffered_tx(42, 0, 1_100)],
        );

        let runtime_task = Arc::clone(&runtime);
        let pipeline_task = Arc::clone(&pipeline);
        let ledger_task = Arc::clone(&ledger);
        let handle = tokio::spawn(async move {
            run(
                runtime_task,
                pipeline_task,
                ledger_task,
                Some(event_tx),
                shutdown_rx,
                GatekeeperCommitLoopConfig {
                    check_interval_ms: 10,
                },
            )
            .await;
        });

        let mut committed_event_seen = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            if ledger.is_committed(&base_mint)
                && pipeline.is_initialized(&base_mint)
                && runtime.runtime_pool_state(&pool_id)
                    == Some(crate::components::gatekeeper::PoolState::Committed)
                && runtime.commit_coordinator().active_buffer_count() == 0
            {
                break;
            }

            if let Ok(Ok(GhostEvent::GatekeeperCommitted {
                pool_amm_id,
                base_mint: event_mint,
                committed_count,
                ..
            })) = tokio::time::timeout(Duration::from_millis(50), event_rx.recv()).await
            {
                if pool_amm_id == pool_id.to_string() && event_mint == base_mint.to_string() {
                    assert_eq!(committed_count, 1);
                    committed_event_seen = true;
                }
            }
        }

        assert!(ledger.is_committed(&base_mint));
        assert!(pipeline.is_initialized(&base_mint));
        assert_eq!(runtime.commit_coordinator().active_buffer_count(), 0);
        assert!(committed_event_seen);

        let _ = shutdown_tx.send(());
        let _ = tokio::time::timeout(Duration::from_secs(1), handle).await;
    }

    #[tokio::test]
    async fn commit_loop_bootstraps_live_pipeline_from_commit_result() {
        let pipeline = Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
        let ledger = Arc::new(ShadowLedger::new());
        let runtime = build_runtime(Arc::clone(&ledger), Arc::clone(&pipeline));
        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        register_pool(&runtime, pool_id, base_mint);
        ledger.set_approval_checker(Arc::new(|_| false));
        runtime.commit_coordinator().stage_history(
            pool_id,
            base_mint,
            30_000_000_000,
            1_073_000_000_000_000,
            vec![sample_buffered_tx(42, 0, 1_100)],
        );

        let handle = tokio::spawn({
            let runtime = Arc::clone(&runtime);
            let pipeline = Arc::clone(&pipeline);
            let ledger = Arc::clone(&ledger);
            async move {
                run(
                    runtime,
                    pipeline,
                    ledger,
                    None,
                    shutdown_rx,
                    GatekeeperCommitLoopConfig {
                        check_interval_ms: 10,
                    },
                )
                .await
            }
        });

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if ledger.is_committed(&base_mint)
                    && pipeline.is_initialized(&base_mint)
                    && runtime.commit_coordinator().active_buffer_count() == 0
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("commit loop did not initialize live pipeline");

        assert!(ledger.get_latest_snapshot(&base_mint).is_none());
        assert!(ledger.get_latest_snapshot_internal(&base_mint).is_some());
        assert!(pipeline.is_initialized(&base_mint));

        let _ = shutdown_tx.send(());
        let _ = tokio::time::timeout(Duration::from_secs(1), handle).await;
    }

    #[test]
    fn pending_live_survives_commit_window() {
        let base_mint = Pubkey::new_unique();
        let coordinator = Arc::new(crate::components::gatekeeper::LauncherCommitCoordinator::new());
        let ledger = Arc::new(ShadowLedger::new());

        let history: Vec<_> = (0..20_000)
            .map(|idx| sample_buffered_tx(10_000 + idx as u64, idx as u32, 1_000 + idx as u64))
            .collect();
        coordinator.stage_history(
            Pubkey::new_unique(),
            base_mint,
            30_000_000_000,
            1_073_000_000_000_000,
            history,
        );

        let worker = {
            let coordinator = Arc::clone(&coordinator);
            let ledger = Arc::clone(&ledger);
            std::thread::spawn(move || coordinator.process_ready_commits(&ledger))
        };

        wait_for_commit_phase(&coordinator, &base_mint, RuntimeCommitPhase::Committing);
        let pending_live = sample_buffered_tx(99_999, 99_999, 99_999);
        assert!(matches!(
            coordinator.add_approved_tx(&base_mint, pending_live.clone()),
            crate::components::gatekeeper::CommitIngressOutcome::PendingLive
        ));

        let committed = worker.join().expect("commit worker join");
        let result = committed.into_iter().next().expect("commit result");
        assert_eq!(result.commit_result.pending_live.len(), 1);
        assert_eq!(result.commit_result.merged_pending_count, 1);
        assert_eq!(result.commit_result.pending_live[0], pending_live);
    }

    #[test]
    fn launcher_commit_failure_recovery_restores_pending_window() {
        let base_mint = Pubkey::new_unique();
        let coordinator = Arc::new(crate::components::gatekeeper::LauncherCommitCoordinator::new());
        let ledger = Arc::new(ShadowLedger::new());

        let history: Vec<_> = (0..5_000)
            .map(|idx| {
                BufferedTx::new(
                    TxKey::new(
                        5_000 + idx as u64,
                        Some(0),
                        Some(idx as u32),
                        Some(Signature::new_unique()),
                        0,
                    )
                    .unwrap(),
                    TradeSide::Buy,
                    1_000_000_000,
                    1_000_000,
                    false,
                    None,
                )
                .unwrap()
            })
            .collect();
        let history_len = history.len();
        coordinator.stage_history(
            Pubkey::new_unique(),
            base_mint,
            30_000_000_000,
            1_073_000_000_000_000,
            history,
        );

        let worker = {
            let coordinator = Arc::clone(&coordinator);
            let ledger = Arc::clone(&ledger);
            std::thread::spawn(move || coordinator.process_ready_commits(&ledger))
        };

        wait_for_commit_phase(&coordinator, &base_mint, RuntimeCommitPhase::Committing);
        assert!(matches!(
            coordinator.add_approved_tx(&base_mint, sample_buffered_tx(88_888, 88_888, 88_888)),
            crate::components::gatekeeper::CommitIngressOutcome::PendingLive
        ));

        let committed = worker.join().expect("commit worker join");
        assert!(
            committed.is_empty(),
            "failed commit must not produce committed outcome"
        );
        assert_eq!(
            coordinator.commit_phase(&base_mint),
            Some(RuntimeCommitPhase::Pending)
        );
        assert_eq!(
            coordinator.buffered_history_count(&base_mint),
            Some(history_len + 1)
        );
    }
}
