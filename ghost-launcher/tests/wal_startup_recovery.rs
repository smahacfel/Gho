//! Integration tests: WAL startup recovery flow (Faza 4)
//!
//! Validates that `replay_shared_wal` correctly restores state across simulated restarts:
//! - committed pools survive restart (CommitPersisted → restore_committed_history_from_wal)
//! - partial (truncated) WAL tail is tolerated without panic or error
//! - snapshot watermark correctly skips pre-snapshot records

use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
use ghost_core::shadow_ledger::BufferedTx;
use ghost_core::shadow_ledger::{
    LivePipeline, LivePipelineConfig, MarketSnapshot, ShadowLedger, TradeSide,
};
use ghost_core::wal::{CommitPersistedRecord, CommitStagedRecord, WalRecord, WalStorageVersion};
use ghost_core::{BaseMint, BondingCurveKey, PoolId, PoolIdentity, TxKey, Wal};
use ghost_launcher::oracle_runtime::{OracleRuntime, OracleRuntimeConfig};
use ghost_launcher::wal_recovery::replay_shared_wal;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::tempdir;

const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const BONK_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";

fn build_runtime(shadow_ledger: Arc<ShadowLedger>) -> Arc<OracleRuntime> {
    let live_pipeline = Arc::new(LivePipeline::with_config(LivePipelineConfig::default()));
    #[allow(deprecated)]
    Arc::new(OracleRuntime::new_with_config(
        Arc::new(HyperPredictionOracle::default()),
        PUMP_PROGRAM_ID.to_string(),
        BONK_PROGRAM_ID.to_string(),
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

fn make_identity() -> PoolIdentity {
    PoolIdentity {
        pool_id: PoolId::from(Pubkey::new_unique()),
        base_mint: BaseMint::from(Pubkey::new_unique()),
        bonding_curve: BondingCurveKey::from(Pubkey::new_unique()),
    }
}

fn make_buffered_tx(timestamp_ms: u64, slot: u64, tx_index: u32) -> BufferedTx {
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

fn make_snapshot(tx_key: TxKey) -> MarketSnapshot {
    let mut snap = MarketSnapshot::new_with_slot(tx_key.timestamp_ms, tx_key.slot);
    snap.tx_key = Some(tx_key);
    snap.tx_count = 1;
    snap.cum_volume_sol = 1.0;
    snap.reserve_quote = 30.0;
    snap.reserve_base = 800_000_000_000.0;
    snap.price_sol_per_token = snap.reserve_quote / snap.reserve_base;
    snap.price_state = ghost_core::shadow_ledger::types::PriceState::Valid;
    snap.market_cap_sol = 120.0;
    snap.bonding_progress_pct = 40.0;
    snap
}

fn append_record_at(wal: &Wal, record: &WalRecord, write_wall_ts_ms: u64) {
    wal.append_with_clock_at(
        record,
        ghost_core::WalRecordClock::default(),
        write_wall_ts_ms,
    )
    .expect("append WAL record");
}

fn current_segment_path(wal_dir: &Path) -> PathBuf {
    let mut segments: Vec<_> = std::fs::read_dir(wal_dir)
        .expect("read WAL dir")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".wal"))
        .map(|entry| entry.path())
        .collect();
    segments.sort();
    segments.pop().expect("WAL segment must exist")
}

fn append_legacy_record(wal_dir: &Path, record: &WalRecord) {
    let seg_path = current_segment_path(wal_dir);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&seg_path)
        .expect("open segment for legacy append");
    let bytes = bincode::serialize(record).expect("serialize legacy record");
    file.write_all(&(bytes.len() as u32).to_le_bytes())
        .expect("write legacy len");
    file.write_all(&bytes).expect("write legacy bytes");
    file.flush().expect("flush legacy segment");
}

/// Test 1: CommitPersisted record survives a simulated restart via WAL replay.
///
/// Flow: write CommitStaged + CommitPersisted to WAL → replay → assert committed_pools_restored == 1.
#[test]
fn staged_commit_survives_restart() {
    let wal_dir = tempdir().expect("wal tempdir");
    let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");

    let identity = make_identity();
    let tx_a = make_buffered_tx(1_000, 10, 0);
    let tx_b = make_buffered_tx(1_200, 11, 1);
    let snap_a = make_snapshot(tx_a.tx_key.clone());
    let snap_b = make_snapshot(tx_b.tx_key.clone());
    let last_tx_key = tx_b.tx_key.clone();

    // Write CommitStaged: initial history
    wal.append(&WalRecord::CommitStaged {
        ts_ms: 1_000,
        slot: 10,
        commit: CommitStagedRecord {
            identity,
            initial_reserve_sol_lamports: 30_000_000_000,
            initial_reserve_tok_units: 1_073_000_000_000_000,
            buffered_history: vec![tx_a, tx_b],
        },
    })
    .expect("append CommitStaged");

    // Write CommitPersisted: pool is fully committed with snapshots
    wal.append(&WalRecord::CommitPersisted {
        ts_ms: 1_500,
        slot: 11,
        commit: CommitPersistedRecord {
            identity,
            last_committed_tx_key: Some(last_tx_key),
            snapshots: vec![snap_a, snap_b],
        },
    })
    .expect("append CommitPersisted");

    wal.flush().expect("wal flush");

    let shadow_ledger = Arc::new(ShadowLedger::new());
    let runtime = build_runtime(Arc::clone(&shadow_ledger));

    let summary = replay_shared_wal(&wal, &runtime, None).expect("WAL replay");

    assert_eq!(
        summary.committed_pools_restored, 1,
        "expected one committed pool restored from WAL"
    );
    assert_eq!(
        summary.skipped_by_watermark, 0,
        "no records should be skipped without watermark"
    );
}

/// Test 2: Truncated WAL tail (simulating crash mid-write) is tolerated.
///
/// Flow: write valid record → append partial header bytes → replay → only valid records restored,
/// no panic, no error.
#[test]
fn partial_wal_tail_is_tolerated() {
    let wal_dir = tempdir().expect("wal tempdir");
    let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");

    let identity = make_identity();
    let tx = make_buffered_tx(2_000, 20, 0);

    wal.append(&WalRecord::CommitStaged {
        ts_ms: 2_000,
        slot: 20,
        commit: CommitStagedRecord {
            identity,
            initial_reserve_sol_lamports: 30_000_000_000,
            initial_reserve_tok_units: 1_073_000_000_000_000,
            buffered_history: vec![tx],
        },
    })
    .expect("append CommitStaged");
    wal.flush().expect("wal flush");

    // Corrupt: append a length header (large value) followed by truncated body — simulates crash
    let segments: Vec<_> = std::fs::read_dir(wal_dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".wal"))
        .collect();
    assert!(!segments.is_empty(), "WAL segment must exist");

    let seg_path = segments[0].path();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&seg_path)
        .expect("open segment for append");

    // Write a 1234-byte length header but only 2 bytes of body → truncated record
    file.write_all(&1234u32.to_le_bytes())
        .expect("write corrupt len");
    file.write_all(&[0xde, 0xad]).expect("write corrupt body");
    file.flush().expect("flush corrupt segment");
    drop(file);

    let shadow_ledger = Arc::new(ShadowLedger::new());
    let runtime = build_runtime(Arc::clone(&shadow_ledger));

    // Replay must not panic or return an error
    let summary = replay_shared_wal(&wal, &runtime, None).expect("WAL replay with truncated tail");

    // The valid CommitStaged record (not CommitPersisted) creates a staged commit
    assert_eq!(
        summary.staged_commits_restored, 1,
        "staged commit before the corruption should be restored"
    );
}

/// Test 3: Snapshot watermark skips pre-snapshot WAL records.
///
/// Flow: write N records with ts_ms < watermark, then M records with ts_ms > watermark →
/// replay with watermark → only post-watermark records processed.
#[test]
fn snapshot_watermark_skips_pre_snapshot_records() {
    let wal_dir = tempdir().expect("wal tempdir");
    let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");

    let watermark_ms: u64 = 5_000;

    // 3 records before watermark (ts_ms <= watermark)
    for i in 0u64..3 {
        let ts_ms = watermark_ms - 100 * (i + 1);
        append_record_at(
            &wal,
            &WalRecord::RawTx {
                ts_ms: watermark_ms - 100 * (i + 1), // 4900, 4800, 4700
                slot: i,
                signature: None,
                raw_tx: vec![i as u8],
            },
            ts_ms,
        );
    }

    // 2 records after watermark (ts_ms > watermark)
    let identity = make_identity();
    let tx_post = make_buffered_tx(watermark_ms + 500, 50, 0);
    let snap_post = make_snapshot(tx_post.tx_key.clone());
    let last_key = tx_post.tx_key.clone();

    append_record_at(
        &wal,
        &WalRecord::CommitStaged {
            ts_ms: watermark_ms + 100,
            slot: 50,
            commit: CommitStagedRecord {
                identity,
                initial_reserve_sol_lamports: 30_000_000_000,
                initial_reserve_tok_units: 1_073_000_000_000_000,
                buffered_history: vec![tx_post],
            },
        },
        watermark_ms + 100,
    );

    append_record_at(
        &wal,
        &WalRecord::CommitPersisted {
            ts_ms: watermark_ms + 600,
            slot: 51,
            commit: CommitPersistedRecord {
                identity,
                last_committed_tx_key: Some(last_key),
                snapshots: vec![snap_post],
            },
        },
        watermark_ms + 600,
    );

    wal.flush().expect("wal flush");

    let shadow_ledger = Arc::new(ShadowLedger::new());
    let runtime = build_runtime(Arc::clone(&shadow_ledger));

    let summary =
        replay_shared_wal(&wal, &runtime, Some(watermark_ms)).expect("WAL replay with watermark");

    assert_eq!(
        summary.skipped_by_watermark, 3,
        "exactly 3 pre-watermark records should be skipped"
    );
    assert_eq!(
        summary.committed_pools_restored, 1,
        "post-watermark CommitPersisted pool should be restored"
    );
    assert_eq!(
        summary.total_records, 2,
        "only 2 records should be counted (post-watermark)"
    );
}

#[test]
fn snapshot_watermark_replays_v2_record_when_write_clock_is_newer_than_payload_ts() {
    let wal_dir = tempdir().expect("wal tempdir");
    let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");
    let watermark_ms: u64 = 5_000;

    append_record_at(
        &wal,
        &WalRecord::RawTx {
            ts_ms: 4_900,
            slot: 1,
            signature: None,
            raw_tx: vec![1],
        },
        5_100,
    );
    wal.flush().expect("wal flush");

    let shadow_ledger = Arc::new(ShadowLedger::new());
    let runtime = build_runtime(Arc::clone(&shadow_ledger));

    let summary =
        replay_shared_wal(&wal, &runtime, Some(watermark_ms)).expect("WAL replay with watermark");

    assert_eq!(summary.skipped_by_watermark, 0);
    assert_eq!(summary.total_records, 1);
    assert_eq!(summary.raw_txs, 1);
}

#[test]
fn snapshot_watermark_handles_mixed_legacy_v1_and_v2_records_by_storage_clock() {
    let wal_dir = tempdir().expect("wal tempdir");
    let wal = Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init");
    let watermark_ms: u64 = 5_000;

    append_legacy_record(
        wal_dir.path(),
        &WalRecord::RawTx {
            ts_ms: 4_800,
            slot: 1,
            signature: None,
            raw_tx: vec![1],
        },
    );
    append_record_at(
        &wal,
        &WalRecord::RawTx {
            ts_ms: 4_900,
            slot: 2,
            signature: None,
            raw_tx: vec![2],
        },
        5_200,
    );
    append_legacy_record(
        wal_dir.path(),
        &WalRecord::RawTx {
            ts_ms: 5_300,
            slot: 3,
            signature: None,
            raw_tx: vec![3],
        },
    );
    wal.flush().expect("wal flush");

    let mut entries = Vec::new();
    wal.replay_from_watermark_entries(watermark_ms, |entry| entries.push(entry))
        .expect("replay watermark entries");
    let replay_entries: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.write_wall_ts_ms > watermark_ms)
        .collect();
    assert_eq!(replay_entries.len(), 2);
    assert_eq!(
        replay_entries[0].storage_version,
        WalStorageVersion::ExplicitWriteClockV2
    );
    assert_eq!(replay_entries[0].record.ts_ms(), 4_900);
    assert_eq!(replay_entries[0].write_wall_ts_ms, 5_200);
    assert_eq!(
        replay_entries[1].storage_version,
        WalStorageVersion::LegacyV1
    );
    assert_eq!(replay_entries[1].record.ts_ms(), 5_300);
    assert_eq!(replay_entries[1].write_wall_ts_ms, 5_300);

    let shadow_ledger = Arc::new(ShadowLedger::new());
    let runtime = build_runtime(Arc::clone(&shadow_ledger));
    let summary =
        replay_shared_wal(&wal, &runtime, Some(watermark_ms)).expect("WAL replay with watermark");

    assert_eq!(summary.skipped_by_watermark, 1);
    assert_eq!(summary.total_records, 2);
    assert_eq!(summary.raw_txs, 2);
}
