use crate::components::gatekeeper::PoolState;
use crate::oracle_runtime::OracleRuntime;
use anyhow::Result;
use ghost_core::shadow_ledger::BufferedTx;
use ghost_core::wal::{
    CommitPersistedRecord, CommitStagedRecord, RollbackReevalSeedRecord,
    ShadowLedgerCurveUpdateRecord, TradeForwardRecord,
};
use ghost_core::{
    CurveFinality, CurveWriteMetadata, MarketSnapshot, PoolIdentity, ShadowLedgerStateConfidence,
    ShadowLedgerWriteReason, ShadowLedgerWriteSource, ShadowLedgerWriteStrength, TxKey, Wal,
    WalRecord, WalReplayEntry,
};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use tracing::warn;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WalReplaySummary {
    pub total_records: u64,
    pub raw_txs: u64,
    pub parsed_events: u64,
    pub decisions: u64,
    pub curve_updates_restored: u64,
    pub staged_commits_restored: u64,
    pub committed_pools_restored: u64,
    pub rollback_seeds_restored: u64,
    pub live_trades_replayed: u64,
    pub live_snapshots_flushed: u64,
    pub skipped_forwarded_trades: u64,
    /// Records skipped because their timestamp pre-dates the snapshot watermark.
    pub skipped_by_watermark: u64,
}

#[derive(Debug, Default)]
struct RecoveredCommitFlow {
    identity: Option<PoolIdentity>,
    initial_reserve_sol_lamports: u64,
    initial_reserve_tok_units: u64,
    staged_history: Vec<BufferedTx>,
    forwarded: Vec<BufferedTx>,
    persisted_snapshots: Option<Vec<MarketSnapshot>>,
    last_committed_tx_key: Option<TxKey>,
}

/// Replay the shared WAL into `oracle_runtime`.
///
/// `skip_before_ms` is an optional snapshot watermark: any WAL record whose
/// write wall-clock is **≤** this value is skipped, because the snapshot
/// already incorporates that state. Passing `None` replays every record
/// (cold start without a snapshot).
pub fn replay_shared_wal(
    wal: &Wal,
    oracle_runtime: &OracleRuntime,
    skip_before_ms: Option<u64>,
) -> Result<WalReplaySummary> {
    let mut summary = WalReplaySummary::default();
    let mut recovered_flows: HashMap<Pubkey, RecoveredCommitFlow> = HashMap::new();
    let mut recovered_rollbacks: HashMap<Pubkey, RollbackReevalSeedRecord> = HashMap::new();
    let shadow_ledger = oracle_runtime.get_shadow_ledger().clone();

    let mut handle_record = |entry: WalReplayEntry| {
        // Delta replay: skip records that are already captured in the snapshot.
        if let Some(watermark) = skip_before_ms {
            if entry.write_wall_ts_ms <= watermark {
                summary.skipped_by_watermark = summary.skipped_by_watermark.saturating_add(1);
                return;
            }
        }

        summary.total_records = summary.total_records.saturating_add(1);
        match entry.record {
            WalRecord::RawTx { .. } => {
                summary.raw_txs = summary.raw_txs.saturating_add(1);
            }
            WalRecord::ParsedEvent { .. } => {
                summary.parsed_events = summary.parsed_events.saturating_add(1);
            }
            WalRecord::Decision { .. } => {
                summary.decisions = summary.decisions.saturating_add(1);
            }
            WalRecord::ShadowLedgerCurveUpdate { slot, update, .. } => {
                apply_curve_update(&shadow_ledger, slot, &update);
                summary.curve_updates_restored = summary.curve_updates_restored.saturating_add(1);
            }
            WalRecord::CommitStaged { commit, .. } => {
                remember_commit_stage(&mut recovered_flows, &mut recovered_rollbacks, commit);
            }
            WalRecord::CommitPersisted { commit, .. } => {
                remember_commit_persisted(&mut recovered_flows, &mut recovered_rollbacks, commit);
            }
            WalRecord::TradeForwarded { trade, .. } => {
                remember_trade_forward(&mut recovered_flows, trade);
            }
            WalRecord::RollbackReevalSeed { rollback, .. } => {
                remember_rollback_seed(&mut recovered_flows, &mut recovered_rollbacks, rollback);
            }
        }
    };

    if let Some(watermark) = skip_before_ms {
        wal.replay_from_watermark_entries(watermark, &mut handle_record)?;
    } else {
        wal.replay_all_entries(&mut handle_record)?;
    }

    for (base_mint, mut flow) in recovered_flows {
        let Some(identity) = flow.identity else {
            summary.skipped_forwarded_trades = summary
                .skipped_forwarded_trades
                .saturating_add(flow.forwarded.len() as u64);
            continue;
        };

        if let Some(snapshots) = flow.persisted_snapshots.take() {
            if oracle_runtime.restore_committed_history_from_wal(
                identity,
                snapshots,
                flow.last_committed_tx_key.clone(),
            ) {
                summary.committed_pools_restored =
                    summary.committed_pools_restored.saturating_add(1);
            }

            let mut post_commit = flow.forwarded;
            if let Some(last_key) = flow.last_committed_tx_key.as_ref() {
                post_commit.retain(|tx| tx.tx_key > *last_key);
            }
            post_commit.sort_by(|lhs, rhs| lhs.tx_key.cmp(&rhs.tx_key));

            let mut replayed_for_mint = 0u64;
            for tx in &post_commit {
                if oracle_runtime.replay_live_tx_from_wal(base_mint, tx) {
                    replayed_for_mint = replayed_for_mint.saturating_add(1);
                }
            }
            if replayed_for_mint > 0 {
                summary.live_trades_replayed = summary
                    .live_trades_replayed
                    .saturating_add(replayed_for_mint);
                summary.live_snapshots_flushed = summary.live_snapshots_flushed.saturating_add(
                    oracle_runtime.flush_replayed_live_mint_from_wal(&base_mint) as u64,
                );
            }
            continue;
        }

        if flow.initial_reserve_sol_lamports == 0 || flow.initial_reserve_tok_units == 0 {
            if !flow.forwarded.is_empty() {
                warn!(
                    base_mint = %base_mint,
                    forwarded = flow.forwarded.len(),
                    "Skipping WAL staged recovery without initial reserves"
                );
                summary.skipped_forwarded_trades = summary
                    .skipped_forwarded_trades
                    .saturating_add(flow.forwarded.len() as u64);
            }
            continue;
        }

        oracle_runtime.restore_runtime_pool_state_from_wal(identity, PoolState::Approved);

        flow.staged_history.extend(flow.forwarded);
        flow.staged_history
            .sort_by(|lhs, rhs| lhs.tx_key.cmp(&rhs.tx_key));
        flow.staged_history
            .dedup_by(|lhs, rhs| lhs.tx_key == rhs.tx_key);

        let staged = oracle_runtime.commit_coordinator().stage_history(
            identity.pool_id.into(),
            base_mint,
            flow.initial_reserve_sol_lamports,
            flow.initial_reserve_tok_units,
            flow.staged_history,
        );
        if staged > 0 {
            summary.staged_commits_restored = summary.staged_commits_restored.saturating_add(1);
        }
    }

    for rollback in recovered_rollbacks.into_values() {
        if oracle_runtime.restore_rollback_seed_from_wal(&rollback) {
            summary.rollback_seeds_restored = summary.rollback_seeds_restored.saturating_add(1);
        }
    }

    Ok(summary)
}

fn remember_commit_stage(
    recovered_flows: &mut HashMap<Pubkey, RecoveredCommitFlow>,
    recovered_rollbacks: &mut HashMap<Pubkey, RollbackReevalSeedRecord>,
    commit: CommitStagedRecord,
) {
    let base_mint: Pubkey = commit.identity.base_mint.into();
    recovered_rollbacks.remove(&base_mint);
    let flow = recovered_flows.entry(base_mint).or_default();
    flow.identity = Some(commit.identity);
    flow.initial_reserve_sol_lamports = commit.initial_reserve_sol_lamports;
    flow.initial_reserve_tok_units = commit.initial_reserve_tok_units;
    flow.staged_history.extend(commit.buffered_history);
}

fn remember_commit_persisted(
    recovered_flows: &mut HashMap<Pubkey, RecoveredCommitFlow>,
    recovered_rollbacks: &mut HashMap<Pubkey, RollbackReevalSeedRecord>,
    commit: CommitPersistedRecord,
) {
    let base_mint: Pubkey = commit.identity.base_mint.into();
    recovered_rollbacks.remove(&base_mint);
    let flow = recovered_flows.entry(base_mint).or_default();
    flow.identity = Some(commit.identity);
    flow.persisted_snapshots = Some(commit.snapshots);
    flow.last_committed_tx_key = commit.last_committed_tx_key;
}

fn remember_trade_forward(
    recovered_flows: &mut HashMap<Pubkey, RecoveredCommitFlow>,
    trade: TradeForwardRecord,
) {
    let base_mint: Pubkey = trade.identity.base_mint.into();
    let flow = recovered_flows.entry(base_mint).or_default();
    flow.identity = Some(trade.identity);
    flow.forwarded.push(trade.tx);
}

fn remember_rollback_seed(
    recovered_flows: &mut HashMap<Pubkey, RecoveredCommitFlow>,
    recovered_rollbacks: &mut HashMap<Pubkey, RollbackReevalSeedRecord>,
    rollback: RollbackReevalSeedRecord,
) {
    let base_mint: Pubkey = rollback.identity.base_mint.into();
    recovered_flows.remove(&base_mint);
    recovered_rollbacks.insert(base_mint, rollback);
}

fn apply_curve_update(
    shadow_ledger: &ghost_core::ShadowLedger,
    slot: u64,
    update: &ShadowLedgerCurveUpdateRecord,
) {
    let base_mint = Pubkey::new_from_array(update.base_mint);
    let bonding_curve = Pubkey::new_from_array(update.bonding_curve);
    let fallback_strength = if slot == 0 {
        ShadowLedgerWriteStrength::BootstrapSeed
    } else if update.curve_data_known {
        ShadowLedgerWriteStrength::ConfirmedBootstrap
    } else {
        ShadowLedgerWriteStrength::Repair
    };
    let fallback_confidence = if slot == 0 {
        ShadowLedgerStateConfidence::Speculative
    } else if update.curve_data_known {
        ShadowLedgerStateConfidence::Observed
    } else {
        ShadowLedgerStateConfidence::Diagnostic
    };
    let fallback_reason = if slot == 0 {
        ShadowLedgerWriteReason::BootstrapSeed
    } else {
        ShadowLedgerWriteReason::WalReplayCurveUpdate
    };
    let fallback_finality = if slot == 0 {
        CurveFinality::Speculative
    } else {
        update.curve_finality
    };
    let metadata = CurveWriteMetadata::new(
        if update.write_source == ShadowLedgerWriteSource::CompatibilityBootstrap {
            ShadowLedgerWriteSource::WalReplayCurve
        } else {
            update.write_source
        },
        if update.write_strength == ShadowLedgerWriteStrength::BootstrapSeed && slot > 0 {
            fallback_strength
        } else {
            update.write_strength
        },
        if update.state_confidence == ShadowLedgerStateConfidence::Speculative && slot > 0 {
            fallback_confidence
        } else {
            update.state_confidence
        },
        if update.write_reason == ShadowLedgerWriteReason::CompatibilityBootstrap {
            fallback_reason
        } else {
            update.write_reason
        },
        (slot > 0).then_some(slot),
        fallback_finality,
    )
    .with_last_update_ts_ms(update.last_update_ts_ms);

    let _ = shadow_ledger.apply_curve_write(Some(base_mint), bonding_curve, update.curve, metadata);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle_runtime::{OracleRuntime, OracleRuntimeConfig};
    use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
    use ghost_core::market_state::BondingCurve;
    use ghost_core::shadow_ledger::{LivePipeline, LivePipelineConfig, ShadowLedger, TradeSide};
    use ghost_core::{BaseMint, BondingCurveKey, PoolId};
    use solana_sdk::signature::Signature;
    use tempfile::tempdir;

    fn build_runtime(
        shadow_ledger: std::sync::Arc<ShadowLedger>,
        live_pipeline: std::sync::Arc<LivePipeline>,
    ) -> std::sync::Arc<OracleRuntime> {
        #[allow(deprecated)]
        std::sync::Arc::new(OracleRuntime::new_with_config(
            std::sync::Arc::new(HyperPredictionOracle::default()),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string(),
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

    fn sample_identity() -> PoolIdentity {
        PoolIdentity {
            pool_id: PoolId::from(Pubkey::new_unique()),
            base_mint: BaseMint::from(Pubkey::new_unique()),
            bonding_curve: BondingCurveKey::from(Pubkey::new_unique()),
        }
    }

    fn sample_buffered_tx(timestamp_ms: u64, slot: u64, tx_index: u32) -> BufferedTx {
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

    fn sample_snapshot(tx_key: TxKey) -> MarketSnapshot {
        let mut snapshot = MarketSnapshot::new_with_slot(tx_key.timestamp_ms, tx_key.slot);
        snapshot.tx_key = Some(tx_key);
        snapshot.tx_count = 1;
        snapshot.cum_volume_sol = 1.0;
        snapshot.reserve_quote = 31.0;
        snapshot.reserve_base = 900_000_000_000.0;
        snapshot.price_sol_per_token = snapshot.reserve_quote / snapshot.reserve_base;
        snapshot.price_state = ghost_core::shadow_ledger::types::PriceState::Valid;
        snapshot.market_cap_sol = 150.0;
        snapshot.bonding_progress_pct = 45.0;
        snapshot
    }

    fn sample_rollback_seed(
        identity: PoolIdentity,
        registered_wall_ts_ms: u64,
    ) -> RollbackReevalSeedRecord {
        RollbackReevalSeedRecord {
            identity,
            quote_mint: Pubkey::new_unique().to_string(),
            amm_program: "pumpfun".to_string(),
            creator: Pubkey::new_unique().to_string(),
            slot: Some(42),
            detected_event_ts_ms: 2_000,
            registered_wall_ts_ms,
            initial_liquidity_sol: Some(30.0),
            signature: Signature::new_unique().to_string(),
            reason: "severe_repair".to_string(),
        }
    }

    fn append_record_at(wal: &Wal, record: &WalRecord, write_wall_ts_ms: u64) {
        wal.append_with_clock_at(
            record,
            ghost_core::WalRecordClock::default(),
            write_wall_ts_ms,
        )
        .expect("append wal record");
    }

    #[test]
    fn replay_restores_staged_commit_for_finalization() {
        let wal_dir = tempdir().expect("wal tempdir");
        let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");
        let identity = sample_identity();
        let tx_a = sample_buffered_tx(1_000, 10, 0);
        let tx_b = sample_buffered_tx(1_100, 11, 1);

        wal.append(&WalRecord::CommitStaged {
            ts_ms: 1_000,
            slot: 10,
            commit: CommitStagedRecord {
                identity,
                initial_reserve_sol_lamports: 30_000_000_000,
                initial_reserve_tok_units: 1_073_000_000_000_000,
                buffered_history: vec![tx_a.clone()],
            },
        })
        .expect("append commit staged");
        wal.append(&WalRecord::TradeForwarded {
            ts_ms: 1_100,
            slot: 11,
            trade: TradeForwardRecord {
                identity,
                tx: tx_b.clone(),
            },
        })
        .expect("append trade forwarded");

        let shadow_ledger = std::sync::Arc::new(ShadowLedger::new());
        let live_pipeline =
            std::sync::Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
        let runtime = build_runtime(std::sync::Arc::clone(&shadow_ledger), live_pipeline);

        let summary = replay_shared_wal(&wal, runtime.as_ref(), None).expect("wal replay");
        assert_eq!(summary.staged_commits_restored, 1);
        assert_eq!(runtime.commit_coordinator().active_buffer_count(), 1);

        let committed = runtime
            .commit_coordinator()
            .process_ready_commits(shadow_ledger.as_ref());
        let result = committed.into_iter().next().expect("commit result");
        assert_eq!(result.commit_result.committed_count, 2);
    }

    #[test]
    fn replay_rollback_seed_suppresses_staged_commit_restore() {
        let wal_dir = tempdir().expect("wal tempdir");
        let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");
        let identity = sample_identity();
        let base_mint: Pubkey = identity.base_mint.into();
        let pool_id: Pubkey = identity.pool_id.into();
        let tx_a = sample_buffered_tx(1_000, 10, 0);

        wal.append(&WalRecord::CommitStaged {
            ts_ms: 1_000,
            slot: 10,
            commit: CommitStagedRecord {
                identity,
                initial_reserve_sol_lamports: 30_000_000_000,
                initial_reserve_tok_units: 1_073_000_000_000_000,
                buffered_history: vec![tx_a],
            },
        })
        .expect("append commit staged");
        wal.append(&WalRecord::RollbackReevalSeed {
            ts_ms: 1_500,
            slot: 42,
            rollback: sample_rollback_seed(identity, 9_999),
        })
        .expect("append rollback seed");

        let shadow_ledger = std::sync::Arc::new(ShadowLedger::new());
        let live_pipeline =
            std::sync::Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
        let runtime = build_runtime(std::sync::Arc::clone(&shadow_ledger), live_pipeline);

        let summary = replay_shared_wal(&wal, runtime.as_ref(), None).expect("wal replay");
        assert_eq!(summary.staged_commits_restored, 0);
        assert_eq!(summary.rollback_seeds_restored, 1);
        assert_eq!(runtime.commit_coordinator().active_buffer_count(), 0);
        assert_eq!(
            runtime.runtime_pool_state(&pool_id),
            Some(PoolState::Tracked)
        );
        assert!(!runtime.approved_pools().is_approved(&pool_id));

        let seeds = runtime.drain_recovered_rollback_seeds();
        assert_eq!(
            seeds.len(),
            1,
            "rollback seed should be queued for router bootstrap"
        );
        assert_eq!(Pubkey::from(seeds[0].identity.pool_id), pool_id);
        assert_eq!(seeds[0].registered_wall_ts_ms, 9_999);
        assert_eq!(runtime.lookup_registered_pool(&base_mint), Some(pool_id));
    }

    #[test]
    fn replay_restores_curve_update_and_live_delta() {
        let wal_dir = tempdir().expect("wal tempdir");
        let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");
        let identity = sample_identity();
        let base_mint: Pubkey = identity.base_mint.into();
        let bonding_curve: Pubkey = identity.bonding_curve.into();
        let committed_key =
            TxKey::new(2_000, Some(20), Some(0), Some(Signature::new_unique()), 0).unwrap();
        let live_tx = sample_buffered_tx(2_100, 21, 1);
        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 900_000_000_000,
            virtual_sol_reserves: 31_000_000_000,
            real_token_reserves: 800_000_000_000,
            real_sol_reserves: 25_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        wal.append(&WalRecord::ShadowLedgerCurveUpdate {
            ts_ms: 1_900,
            slot: 19,
            update: ShadowLedgerCurveUpdateRecord {
                base_mint: base_mint.to_bytes(),
                bonding_curve: bonding_curve.to_bytes(),
                curve,
                curve_data_known: true,
                curve_finality: CurveFinality::Provisional,
                last_update_ts_ms: 1_900,
                write_source: ShadowLedgerWriteSource::WalReplayCurve,
                write_strength: ShadowLedgerWriteStrength::Repair,
                state_confidence: ShadowLedgerStateConfidence::Diagnostic,
                write_reason: ShadowLedgerWriteReason::WalReplayCurveUpdate,
            },
        })
        .expect("append curve update");
        wal.append(&WalRecord::CommitPersisted {
            ts_ms: 2_000,
            slot: 20,
            commit: CommitPersistedRecord {
                identity,
                last_committed_tx_key: Some(committed_key.clone()),
                snapshots: vec![sample_snapshot(committed_key)],
            },
        })
        .expect("append commit persisted");
        wal.append(&WalRecord::TradeForwarded {
            ts_ms: 2_100,
            slot: 21,
            trade: TradeForwardRecord {
                identity,
                tx: live_tx.clone(),
            },
        })
        .expect("append trade forwarded");

        let shadow_ledger = std::sync::Arc::new(ShadowLedger::new());
        let live_pipeline =
            std::sync::Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
        let runtime = build_runtime(std::sync::Arc::clone(&shadow_ledger), live_pipeline);

        let summary = replay_shared_wal(&wal, runtime.as_ref(), None).expect("wal replay");
        assert_eq!(summary.curve_updates_restored, 1);
        assert_eq!(summary.committed_pools_restored, 1);
        assert_eq!(summary.live_trades_replayed, 0);
        assert_eq!(
            summary.live_snapshots_flushed, 0,
            "PR 8 replay restores committed state without local live-delta flushes"
        );

        let restored_curve = shadow_ledger
            .get_curve(&bonding_curve)
            .expect("curve should be restored");
        assert_eq!(restored_curve, curve);
        assert!(shadow_ledger.is_committed(&base_mint));
        let snapshots = shadow_ledger
            .get_snapshots_internal(&base_mint)
            .expect("snapshots should exist");
        assert_eq!(
            snapshots.len(),
            1,
            "only the committed snapshot should be restored when live replay stays diagnostic-only"
        );
    }

    #[test]
    fn replay_restores_genesis_seed_curve_update() {
        let wal_dir = tempdir().expect("wal tempdir");
        let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");
        let identity = sample_identity();
        let base_mint: Pubkey = identity.base_mint.into();
        let bonding_curve: Pubkey = identity.bonding_curve.into();
        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_073_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 0,
            real_sol_reserves: 0,
            token_total_supply: 1_073_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        wal.append(&WalRecord::ShadowLedgerCurveUpdate {
            ts_ms: 1_000,
            slot: 0,
            update: ShadowLedgerCurveUpdateRecord {
                base_mint: base_mint.to_bytes(),
                bonding_curve: bonding_curve.to_bytes(),
                curve,
                curve_data_known: false,
                curve_finality: CurveFinality::Speculative,
                last_update_ts_ms: 1_000,
                write_source: ShadowLedgerWriteSource::WalReplayCurve,
                write_strength: ShadowLedgerWriteStrength::BootstrapSeed,
                state_confidence: ShadowLedgerStateConfidence::Speculative,
                write_reason: ShadowLedgerWriteReason::BootstrapSeed,
            },
        })
        .expect("append seed curve");

        let shadow_ledger = std::sync::Arc::new(ShadowLedger::new());
        let live_pipeline =
            std::sync::Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
        let runtime = build_runtime(std::sync::Arc::clone(&shadow_ledger), live_pipeline);

        let summary = replay_shared_wal(&wal, runtime.as_ref(), None).expect("wal replay");
        assert_eq!(summary.curve_updates_restored, 1);

        let (restored_curve, curve_known) = shadow_ledger
            .get_curve_with_known(&bonding_curve)
            .expect("seed curve should be restored");
        assert_eq!(restored_curve, curve);
        assert!(!curve_known, "genesis seed must stay non-authoritative");
    }

    #[test]
    fn replay_duplicate_bootstrap_seed_is_noop_under_storage_arbitration() {
        let wal_dir = tempdir().expect("wal tempdir");
        let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");
        let identity = sample_identity();
        let base_mint: Pubkey = identity.base_mint.into();
        let bonding_curve: Pubkey = identity.bonding_curve.into();
        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_073_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 0,
            real_sol_reserves: 0,
            token_total_supply: 1_073_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        wal.append(&WalRecord::ShadowLedgerCurveUpdate {
            ts_ms: 1_000,
            slot: 0,
            update: ShadowLedgerCurveUpdateRecord {
                base_mint: base_mint.to_bytes(),
                bonding_curve: bonding_curve.to_bytes(),
                curve,
                curve_data_known: false,
                curve_finality: CurveFinality::Speculative,
                last_update_ts_ms: 1_000,
                write_source: ShadowLedgerWriteSource::WalReplayCurve,
                write_strength: ShadowLedgerWriteStrength::BootstrapSeed,
                state_confidence: ShadowLedgerStateConfidence::Speculative,
                write_reason: ShadowLedgerWriteReason::BootstrapSeed,
            },
        })
        .expect("append duplicate bootstrap seed");

        let shadow_ledger = std::sync::Arc::new(ShadowLedger::new());
        let _ = shadow_ledger.apply_curve_write(
            Some(base_mint),
            bonding_curve,
            curve,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::SeerBootstrap,
                ShadowLedgerWriteStrength::BootstrapSeed,
                ShadowLedgerStateConfidence::Speculative,
                ShadowLedgerWriteReason::BootstrapSeed,
                None,
                CurveFinality::Speculative,
            )
            .with_last_update_ts_ms(500),
        );
        let live_pipeline =
            std::sync::Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
        let runtime = build_runtime(std::sync::Arc::clone(&shadow_ledger), live_pipeline);

        let summary = replay_shared_wal(&wal, runtime.as_ref(), None).expect("wal replay");
        assert_eq!(summary.curve_updates_restored, 1);

        let stored = shadow_ledger
            .get_old(&bonding_curve)
            .expect("bootstrap curve should remain present");
        assert_eq!(stored.curve, curve);
        assert_eq!(stored.write_source, ShadowLedgerWriteSource::SeerBootstrap);
        assert_eq!(
            stored.write_strength,
            ShadowLedgerWriteStrength::BootstrapSeed
        );
    }

    /// Verify the watermark path:
    ///
    /// 1. WAL contains a pre-snapshot curve update (ts_ms ≤ watermark)
    /// 2. WAL contains a post-snapshot curve update (ts_ms > watermark)
    /// 3. After `restore_from_disk` the ledger already has the pre-snapshot state
    /// 4. `replay_shared_wal` with the watermark must skip the pre-snapshot record
    ///    and apply only the post-snapshot delta
    ///
    /// This proves that `restart → snapshot restore + WAL delta replay` gives the
    /// same logical state as `snapshot state ∪ WAL delta`, without double-applying
    /// records already captured in the snapshot.
    #[test]
    fn test_watermark_skips_pre_snapshot_curve_updates() {
        let wal_dir = tempdir().expect("wal tempdir");
        let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");

        let identity = sample_identity();
        let base_mint: Pubkey = identity.base_mint.into();
        let bonding_curve: Pubkey = identity.bonding_curve.into();

        // Two distinct curve states: one before the snapshot, one after.
        let pre_curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 900_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 800_000_000_000,
            real_sol_reserves: 25_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        let post_curve = BondingCurve {
            virtual_sol_reserves: 31_500_000_000, // higher reserves — distinct from pre
            ..pre_curve
        };

        // Timeline:
        //  ts=1_000  — pre-snapshot curve update written to WAL
        //  ts=5_000  — snapshot written (watermark)
        //  ts=9_000  — post-snapshot curve update written to WAL

        let watermark_ms: u64 = 5_000;

        // Pre-snapshot WAL record.
        append_record_at(
            &wal,
            &WalRecord::ShadowLedgerCurveUpdate {
                ts_ms: 1_000,
                slot: 10,
                update: ShadowLedgerCurveUpdateRecord {
                    base_mint: base_mint.to_bytes(),
                    bonding_curve: bonding_curve.to_bytes(),
                    curve: pre_curve,
                    curve_data_known: true,
                    curve_finality: CurveFinality::Provisional,
                    last_update_ts_ms: 1_000,
                    write_source: ShadowLedgerWriteSource::WalReplayCurve,
                    write_strength: ShadowLedgerWriteStrength::Repair,
                    state_confidence: ShadowLedgerStateConfidence::Diagnostic,
                    write_reason: ShadowLedgerWriteReason::WalReplayCurveUpdate,
                },
            },
            1_000,
        );

        // Post-snapshot WAL record.
        append_record_at(
            &wal,
            &WalRecord::ShadowLedgerCurveUpdate {
                ts_ms: 9_000,
                slot: 20,
                update: ShadowLedgerCurveUpdateRecord {
                    base_mint: base_mint.to_bytes(),
                    bonding_curve: bonding_curve.to_bytes(),
                    curve: post_curve,
                    curve_data_known: true,
                    curve_finality: CurveFinality::Provisional,
                    last_update_ts_ms: 9_000,
                    write_source: ShadowLedgerWriteSource::WalReplayCurve,
                    write_strength: ShadowLedgerWriteStrength::Repair,
                    state_confidence: ShadowLedgerStateConfidence::Diagnostic,
                    write_reason: ShadowLedgerWriteReason::WalReplayCurveUpdate,
                },
            },
            9_000,
        );

        // Simulate snapshot restore: build a fresh ledger that already contains
        // `pre_curve` (as if restored from disk).  This is what `restore_from_disk`
        // would produce after the snapshot was written at ts=5_000.
        let shadow_ledger = std::sync::Arc::new(ShadowLedger::new());
        shadow_ledger.register_curve_alias(base_mint, bonding_curve);
        let _ = shadow_ledger.apply_curve_write(
            Some(base_mint),
            bonding_curve,
            pre_curve,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::AccountUpdate,
                ShadowLedgerWriteStrength::Repair,
                ShadowLedgerStateConfidence::Observed,
                ShadowLedgerWriteReason::DirectAccountUpdate,
                Some(10),
                CurveFinality::Provisional,
            )
            .with_last_update_ts_ms(1_000),
        );

        let live_pipeline =
            std::sync::Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
        let runtime = build_runtime(std::sync::Arc::clone(&shadow_ledger), live_pipeline);

        // Replay with snapshot watermark — pre-snapshot record must be skipped.
        let summary =
            replay_shared_wal(&wal, runtime.as_ref(), Some(watermark_ms)).expect("wal replay");

        assert_eq!(
            summary.skipped_by_watermark, 1,
            "pre-snapshot record (ts=1_000 ≤ watermark=5_000) must be skipped"
        );
        assert_eq!(
            summary.curve_updates_restored, 1,
            "post-snapshot delta (ts=9_000 > watermark=5_000) must be applied"
        );

        // Final curve must be `post_curve` — the delta was applied on top of the
        // snapshot state; the pre-snapshot record was not double-applied.
        let final_curve = shadow_ledger
            .get_curve(&bonding_curve)
            .expect("curve must exist after replay");
        assert_eq!(
            final_curve, post_curve,
            "final state must reflect snapshot ∪ delta; pre-snapshot record must not overwrite it"
        );
    }

    #[test]
    fn test_watermark_filters_by_write_wall_clock_not_legacy_payload_ts() {
        let wal_dir = tempdir().expect("wal tempdir");
        let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");

        let identity = sample_identity();
        let base_mint: Pubkey = identity.base_mint.into();
        let bonding_curve: Pubkey = identity.bonding_curve.into();
        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 910_000_000_000,
            virtual_sol_reserves: 30_500_000_000,
            real_token_reserves: 805_000_000_000,
            real_sol_reserves: 25_100_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        append_record_at(
            &wal,
            &WalRecord::ShadowLedgerCurveUpdate {
                ts_ms: 1_000,
                slot: 10,
                update: ShadowLedgerCurveUpdateRecord {
                    base_mint: base_mint.to_bytes(),
                    bonding_curve: bonding_curve.to_bytes(),
                    curve,
                    curve_data_known: true,
                    curve_finality: CurveFinality::Provisional,
                    last_update_ts_ms: 1_000,
                    write_source: ShadowLedgerWriteSource::WalReplayCurve,
                    write_strength: ShadowLedgerWriteStrength::Repair,
                    state_confidence: ShadowLedgerStateConfidence::Diagnostic,
                    write_reason: ShadowLedgerWriteReason::WalReplayCurveUpdate,
                },
            },
            9_000,
        );

        let shadow_ledger = std::sync::Arc::new(ShadowLedger::new());
        let live_pipeline =
            std::sync::Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
        let runtime = build_runtime(std::sync::Arc::clone(&shadow_ledger), live_pipeline);

        let summary = replay_shared_wal(&wal, runtime.as_ref(), Some(5_000)).expect("wal replay");
        assert_eq!(
            summary.skipped_by_watermark, 0,
            "delta replay must not skip V2 records solely because their legacy payload ts is stale"
        );
        assert_eq!(summary.curve_updates_restored, 1);
        assert_eq!(
            shadow_ledger
                .get_curve(&bonding_curve)
                .expect("curve should be restored"),
            curve
        );
    }

    /// Demonstrate that snapshot + delta replay processes significantly fewer
    /// WAL records than a full replay, proving the startup-time advantage.
    ///
    /// We build a WAL with `N_PRE` pre-snapshot records and `N_POST` post-snapshot
    /// records, write a snapshot at the watermark, then compare:
    ///
    /// - Full replay:            processes all `N_PRE + N_POST` records.
    /// - Snapshot + delta:       skips `N_PRE`, processes only `N_POST`.
    ///
    /// The record-processing counts are verified deterministically (no timing
    /// dependency), proving the O(delta) vs O(total) startup-cost difference.
    #[test]
    fn test_snapshot_delta_replay_processes_fewer_records_than_full_replay() {
        let wal_dir = tempdir().expect("wal tempdir");
        let snap_dir = tempfile::TempDir::new().expect("snap tempdir");
        let wal = Wal::new(wal_dir.path(), 3_600_000, 3_600_000).expect("wal init");

        const N_PRE: u64 = 200; // records before snapshot
        const N_POST: u64 = 20; // delta records after snapshot
        let watermark_ms: u64 = N_PRE + 1;

        // Write N_PRE curve update records to WAL (pre-snapshot).
        let mut pairs: Vec<(Pubkey, Pubkey)> = Vec::new();
        for i in 0..N_PRE {
            let base = Pubkey::new_unique();
            let bc = Pubkey::new_unique();
            pairs.push((base, bc));
            append_record_at(
                &wal,
                &WalRecord::ShadowLedgerCurveUpdate {
                    ts_ms: i + 1, // ts in [1, N_PRE], all ≤ watermark
                    slot: i + 1,
                    update: ShadowLedgerCurveUpdateRecord {
                        base_mint: base.to_bytes(),
                        bonding_curve: bc.to_bytes(),
                        curve: BondingCurve {
                            discriminator: 0,
                            virtual_token_reserves: 900_000_000_000,
                            virtual_sol_reserves: 30_000_000_000u64.wrapping_add(i * 1_000),
                            real_token_reserves: 800_000_000_000,
                            real_sol_reserves: 25_000_000_000,
                            token_total_supply: 1_000_000_000_000,
                            complete: 0,
                            _padding: [0; 7],
                        },
                        curve_data_known: true,
                        curve_finality: CurveFinality::Provisional,
                        last_update_ts_ms: i + 1,
                        write_source: ShadowLedgerWriteSource::WalReplayCurve,
                        write_strength: ShadowLedgerWriteStrength::Repair,
                        state_confidence: ShadowLedgerStateConfidence::Diagnostic,
                        write_reason: ShadowLedgerWriteReason::WalReplayCurveUpdate,
                    },
                },
                i + 1,
            );
        }

        // Build the snapshot manually with `written_at_ms = watermark_ms` so that
        // `restore_from_disk` returns the same watermark we use for WAL filtering.
        // Using `snapshot_to_disk` would stamp `now_ms()` (real epoch ≈ 10^12),
        // which is larger than all synthetic WAL timestamps, causing every WAL
        // record to be skipped — which would break the delta assertions.
        {
            use ghost_core::market_state::ShadowBondingCurve;
            use ghost_core::shadow_ledger::disk_snapshot::{
                snapshot_file_path, write_snapshot_atomic, DiskSnapshot, SNAPSHOT_FORMAT_VERSION,
            };
            let sample_bc = BondingCurve {
                discriminator: 0,
                virtual_token_reserves: 900_000_000_000,
                virtual_sol_reserves: 30_000_000_000,
                real_token_reserves: 800_000_000_000,
                real_sol_reserves: 25_000_000_000,
                token_total_supply: 1_000_000_000_000,
                complete: 0,
                _padding: [0; 7],
            };
            let snap = DiskSnapshot {
                version: SNAPSHOT_FORMAT_VERSION,
                written_at_ms: watermark_ms, // matches WAL timeline, not wall clock
                curves_count: pairs.len(),
                curves: pairs
                    .iter()
                    .map(|(_, bc)| {
                        (
                            bc.to_bytes(),
                            ShadowBondingCurve::new_with_known(sample_bc, 1, true),
                        )
                    })
                    .collect(),
                curve_keys_by_base_mint: pairs
                    .iter()
                    .map(|(base, bc)| (base.to_bytes(), bc.to_bytes()))
                    .collect(),
                snapshots: vec![],
                snapshot_commit_state: vec![],
                bva_archives: vec![],
            };
            let path = snapshot_file_path(snap_dir.path(), watermark_ms);
            write_snapshot_atomic(&snap, &path).expect("write manual snapshot");
        }

        // Write N_POST delta records to WAL (post-snapshot).
        for i in 0..N_POST {
            let ts = watermark_ms + i + 1; // ts > watermark
            let (base, bc) = pairs[i as usize];
            append_record_at(
                &wal,
                &WalRecord::ShadowLedgerCurveUpdate {
                    ts_ms: ts,
                    slot: ts,
                    update: ShadowLedgerCurveUpdateRecord {
                        base_mint: base.to_bytes(),
                        bonding_curve: bc.to_bytes(),
                        curve: BondingCurve {
                            discriminator: 0,
                            virtual_token_reserves: 900_000_000_000,
                            virtual_sol_reserves: 31_000_000_000, // updated reserves
                            real_token_reserves: 800_000_000_000,
                            real_sol_reserves: 25_000_000_000,
                            token_total_supply: 1_000_000_000_000,
                            complete: 0,
                            _padding: [0; 7],
                        },
                        curve_data_known: true,
                        curve_finality: CurveFinality::Provisional,
                        last_update_ts_ms: ts,
                        write_source: ShadowLedgerWriteSource::WalReplayCurve,
                        write_strength: ShadowLedgerWriteStrength::Repair,
                        state_confidence: ShadowLedgerStateConfidence::Diagnostic,
                        write_reason: ShadowLedgerWriteReason::WalReplayCurveUpdate,
                    },
                },
                ts,
            );
        }

        // --- Full WAL replay (no watermark) — cold start baseline ---
        let shadow_full = std::sync::Arc::new(ShadowLedger::new());
        let lp_full = std::sync::Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
        let rt_full = build_runtime(std::sync::Arc::clone(&shadow_full), lp_full);
        let t_cold = std::time::Instant::now();
        let summary_full = replay_shared_wal(&wal, rt_full.as_ref(), None).expect("full replay");
        let cold_elapsed = t_cold.elapsed();

        assert_eq!(
            summary_full.curve_updates_restored,
            N_PRE + N_POST,
            "full replay must process all records"
        );
        assert_eq!(
            summary_full.skipped_by_watermark, 0,
            "full replay must not skip any records"
        );

        // --- Snapshot restore + delta replay — warm start ---
        let (shadow_delta, restore_stats) =
            ShadowLedger::restore_from_disk(snap_dir.path()).expect("restore from snapshot");
        assert_eq!(
            restore_stats.curves_loaded, N_PRE as usize,
            "snapshot must carry all pre-snapshot curves"
        );

        let shadow_delta = std::sync::Arc::new(shadow_delta);
        let lp_delta =
            std::sync::Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
        let rt_delta = build_runtime(std::sync::Arc::clone(&shadow_delta), lp_delta);

        let t_warm = std::time::Instant::now();
        let summary_delta =
            replay_shared_wal(&wal, rt_delta.as_ref(), Some(restore_stats.written_at_ms))
                .expect("delta replay");
        let warm_elapsed = t_warm.elapsed();

        assert_eq!(
            summary_delta.skipped_by_watermark, N_PRE,
            "delta replay must skip all N_PRE pre-snapshot records"
        );
        assert_eq!(
            summary_delta.curve_updates_restored, N_POST,
            "delta replay must process only N_POST post-snapshot records"
        );

        // Both paths converge to the same logical state for the N_POST updated curves.
        for i in 0..N_POST as usize {
            let (_base, bc) = pairs[i];
            let curve_full = shadow_full.get_curve(&bc).expect("curve in full replay");
            let curve_delta = shadow_delta.get_curve(&bc).expect("curve in delta replay");
            assert_eq!(
                curve_full, curve_delta,
                "full replay and snapshot+delta must yield identical state for curve {i}"
            );
        }

        // Startup time criterion: warm restart (snapshot restore + N_POST delta records)
        // must be strictly faster than cold start (full replay of N_PRE + N_POST records).
        // N_POST/N_TOTAL = 20/220 ≈ 9% of records — the WAL replay alone is O(delta),
        // so warm_elapsed must be well below cold_elapsed.
        assert!(
            warm_elapsed < cold_elapsed,
            "warm restart ({warm_elapsed:?}) must be faster than cold full-replay ({cold_elapsed:?}): \
             delta={N_POST} records vs full={} records",
            N_PRE + N_POST
        );
    }

    /// Verify that the periodic snapshot task produces files when called
    /// directly (confirms the write-path used by the background task is sound).
    ///
    /// We simulate two "ticks" of the periodic task by calling `snapshot_to_disk`
    /// twice with a distinct watermark between them, then confirm:
    /// - two snapshot files are created
    /// - rotation leaves only `keep_n` files
    /// - the snapshot survives a restore call
    #[test]
    fn test_periodic_snapshot_task_produces_files() {
        use ghost_core::shadow_ledger::disk_snapshot::{list_snapshot_files, now_ms};
        use std::thread;
        use std::time::Duration;
        use tempfile::TempDir;

        let snap_dir = TempDir::new().expect("snap tempdir");
        let ledger = ShadowLedger::new();

        // Populate ledger.
        for i in 0u64..10 {
            let bc = Pubkey::new_unique();
            let _ = ledger.apply_curve_write(
                None,
                bc,
                BondingCurve {
                    discriminator: 0,
                    virtual_token_reserves: 900_000_000_000,
                    virtual_sol_reserves: 30_000_000_000u64.wrapping_add(i * 1_000),
                    real_token_reserves: 800_000_000_000,
                    real_sol_reserves: 25_000_000_000,
                    token_total_supply: 1_000_000_000_000,
                    complete: 0,
                    _padding: [0; 7],
                },
                CurveWriteMetadata::new(
                    ShadowLedgerWriteSource::AccountUpdate,
                    ShadowLedgerWriteStrength::Repair,
                    ShadowLedgerStateConfidence::Observed,
                    ShadowLedgerWriteReason::DirectAccountUpdate,
                    Some(i + 1),
                    CurveFinality::Provisional,
                )
                .with_last_update_ts_ms(i + 1),
            );
        }

        // Tick 1 — simulate first periodic snapshot.
        let stats1 = ledger
            .snapshot_to_disk(snap_dir.path())
            .expect("first periodic snapshot");
        assert_eq!(stats1.curves_written, 10, "tick 1 must write all curves");

        // Small sleep to ensure a different millisecond timestamp for tick 2.
        thread::sleep(Duration::from_millis(2));

        // Tick 2 — simulate second periodic snapshot.
        let stats2 = ledger
            .snapshot_to_disk(snap_dir.path())
            .expect("second periodic snapshot");
        assert_eq!(stats2.curves_written, 10, "tick 2 must write all curves");

        // Two distinct files should now exist.
        let files_before_rotate = list_snapshot_files(snap_dir.path()).expect("list before rotate");
        assert_eq!(
            files_before_rotate.len(),
            2,
            "periodic task must produce one file per tick"
        );

        // Rotation (keep_n=1) leaves only the newest — mirrors the production loop.
        ShadowLedger::rotate_snapshots(snap_dir.path(), 1).expect("rotate");
        let files_after_rotate = list_snapshot_files(snap_dir.path()).expect("list after rotate");
        assert_eq!(
            files_after_rotate.len(),
            1,
            "rotation must retain only the newest snapshot"
        );

        // The surviving snapshot must be restorable with full curve count.
        let (restored, _stats) =
            ShadowLedger::restore_from_disk(snap_dir.path()).expect("restore after rotate");
        assert_eq!(
            restored.len(),
            10,
            "restored ledger must contain all curves after rotation"
        );
    }

    /// End-to-end: snapshot restore + WAL delta replay covering all commit-path variants.
    ///
    /// Scenario:
    ///   ts=1000  ShadowLedgerCurveUpdate    ← pre-snapshot, in snapshot
    ///   ts=2000  CommitStaged               ← pre-snapshot, in snapshot
    ///   ts=3000  CommitPersisted            ← pre-snapshot, in snapshot
    ///   ts=4000  TradeForwarded             ← pre-snapshot, in snapshot
    ///   [snapshot written with watermark=5000, contains curve state]
    ///   ts=6000  ShadowLedgerCurveUpdate    ← post-snapshot DELTA
    ///
    /// After restore_from_disk + replay_shared_wal(watermark=5000):
    /// - exactly 4 records must be skipped (pre-snapshot)
    /// - exactly 1 record must be applied (post-snapshot delta)
    /// - the curve from the snapshot is present and updated by the delta
    #[test]
    fn test_e2e_snapshot_restore_plus_wal_delta_all_commit_types() {
        use ghost_core::market_state::ShadowBondingCurve;
        use ghost_core::shadow_ledger::ShadowLedger;
        use std::sync::Arc;
        use tempfile::TempDir;

        let wal_dir = TempDir::new().expect("wal tempdir");
        let snap_dir = TempDir::new().expect("snap tempdir");

        let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");
        let identity = sample_identity();
        let base_mint: Pubkey = identity.base_mint.into();
        let bonding_curve_key: Pubkey = identity.bonding_curve.into();

        // Pre-snapshot curve state (will be in the snapshot).
        let curve_v1 = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_073_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 860_000_000_000_000,
            real_sol_reserves: 24_000_000_000,
            token_total_supply: 1_073_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        // Post-snapshot curve delta (updated virtual_sol_reserves).
        let curve_v2 = BondingCurve {
            virtual_sol_reserves: 31_000_000_000,
            ..curve_v1
        };

        let tx_a = sample_buffered_tx(1_000, 10, 0);
        let committed_key = TxKey::new(
            3_000,
            Some(30),
            Some(0),
            Some(solana_sdk::signature::Signature::new_unique()),
            0,
        )
        .unwrap();

        // ── Pre-snapshot WAL records ────────────────────────────────────────
        append_record_at(
            &wal,
            &WalRecord::ShadowLedgerCurveUpdate {
                ts_ms: 1_000,
                slot: 10,
                update: ShadowLedgerCurveUpdateRecord {
                    base_mint: base_mint.to_bytes(),
                    bonding_curve: bonding_curve_key.to_bytes(),
                    curve: curve_v1,
                    curve_data_known: true,
                    curve_finality: CurveFinality::Provisional,
                    last_update_ts_ms: 1_000,
                    write_source: ShadowLedgerWriteSource::WalReplayCurve,
                    write_strength: ShadowLedgerWriteStrength::Repair,
                    state_confidence: ShadowLedgerStateConfidence::Diagnostic,
                    write_reason: ShadowLedgerWriteReason::WalReplayCurveUpdate,
                },
            },
            1_000,
        );

        append_record_at(
            &wal,
            &WalRecord::CommitStaged {
                ts_ms: 2_000,
                slot: 20,
                commit: CommitStagedRecord {
                    identity,
                    initial_reserve_sol_lamports: 30_000_000_000,
                    initial_reserve_tok_units: 1_073_000_000_000_000,
                    buffered_history: vec![tx_a.clone()],
                },
            },
            2_000,
        );

        append_record_at(
            &wal,
            &WalRecord::CommitPersisted {
                ts_ms: 3_000,
                slot: 30,
                commit: CommitPersistedRecord {
                    identity,
                    last_committed_tx_key: Some(committed_key.clone()),
                    snapshots: vec![sample_snapshot(committed_key.clone())],
                },
            },
            3_000,
        );

        append_record_at(
            &wal,
            &WalRecord::TradeForwarded {
                ts_ms: 4_000,
                slot: 40,
                trade: TradeForwardRecord {
                    identity,
                    tx: tx_a.clone(),
                },
            },
            4_000,
        );

        // ── Build and write the disk snapshot (watermark = 5000) ─────────────
        // The snapshot captures curve_v1 state. We write it manually so the
        // watermark is exactly 5000 (not wall-clock now_ms() which would be ~10^12).
        {
            use ghost_core::shadow_ledger::disk_snapshot::{
                snapshot_file_path, write_snapshot_atomic, DiskSnapshot, SNAPSHOT_FORMAT_VERSION,
            };
            let snapshot = DiskSnapshot {
                version: SNAPSHOT_FORMAT_VERSION,
                written_at_ms: 5_000,
                curves_count: 1,
                curves: vec![(
                    bonding_curve_key.to_bytes(),
                    ShadowBondingCurve::new(curve_v1, 10),
                )],
                curve_keys_by_base_mint: vec![(base_mint.to_bytes(), bonding_curve_key.to_bytes())],
                snapshots: vec![],
                snapshot_commit_state: vec![],
                bva_archives: vec![],
            };
            let snap_path = snapshot_file_path(snap_dir.path(), 5_000);
            write_snapshot_atomic(&snapshot, &snap_path)
                .expect("write snapshot with watermark=5000");
        }

        // ── Post-snapshot WAL delta ──────────────────────────────────────────
        append_record_at(
            &wal,
            &WalRecord::ShadowLedgerCurveUpdate {
                ts_ms: 6_000,
                slot: 60,
                update: ShadowLedgerCurveUpdateRecord {
                    base_mint: base_mint.to_bytes(),
                    bonding_curve: bonding_curve_key.to_bytes(),
                    curve: curve_v2,
                    curve_data_known: true,
                    curve_finality: CurveFinality::Provisional,
                    last_update_ts_ms: 6_000,
                    write_source: ShadowLedgerWriteSource::WalReplayCurve,
                    write_strength: ShadowLedgerWriteStrength::Repair,
                    state_confidence: ShadowLedgerStateConfidence::Diagnostic,
                    write_reason: ShadowLedgerWriteReason::WalReplayCurveUpdate,
                },
            },
            6_000,
        );

        // ── Restore from disk snapshot ──────────────────────────────────────
        let (ledger_after_restore, restore_stats) =
            ShadowLedger::restore_from_disk(snap_dir.path()).expect("restore_from_disk");
        assert_eq!(restore_stats.written_at_ms, 5_000, "watermark must be 5000");
        assert_eq!(
            restore_stats.curves_loaded, 1,
            "snapshot must contain 1 curve"
        );

        // Curve v1 is present immediately after restore (before WAL replay).
        let after_restore = ledger_after_restore
            .get_curve(&bonding_curve_key)
            .expect("curve must be present after restore");
        assert_eq!(
            after_restore.virtual_sol_reserves, curve_v1.virtual_sol_reserves,
            "restored curve must be v1"
        );

        // ── WAL delta replay (watermark = 5000) ─────────────────────────────
        let live_pipeline = Arc::new(ghost_core::shadow_ledger::LivePipeline::with_config(
            ghost_core::shadow_ledger::LivePipelineConfig::default(),
        ));
        let ledger_arc = Arc::new(ledger_after_restore);
        let runtime = build_runtime(Arc::clone(&ledger_arc), live_pipeline);

        let summary =
            replay_shared_wal(&wal, runtime.as_ref(), Some(5_000)).expect("wal delta replay");

        // 4 pre-snapshot records must be skipped.
        assert_eq!(
            summary.skipped_by_watermark, 4,
            "CommitStaged + CommitPersisted + TradeForwarded + CurveUpdateV1 must all be skipped (ts ≤ 5000)"
        );
        // 1 post-snapshot delta must be applied.
        assert_eq!(
            summary.curve_updates_restored, 1,
            "exactly 1 post-snapshot curve update must be replayed"
        );

        // After delta replay the curve must be v2 (updated by the post-snapshot record).
        let after_replay = ledger_arc
            .get_curve(&bonding_curve_key)
            .expect("curve must exist after delta replay");
        assert_eq!(
            after_replay.virtual_sol_reserves, curve_v2.virtual_sol_reserves,
            "delta replay must update curve to v2"
        );
    }
}
