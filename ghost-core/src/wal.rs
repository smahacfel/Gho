use crate::event_time::EventTimeMetadata;
use crate::market_state::{
    BondingCurve, ShadowLedgerStateConfidence, ShadowLedgerWriteReason, ShadowLedgerWriteSource,
    ShadowLedgerWriteStrength,
};
use crate::pool_identity::PoolIdentity;
use crate::shadow_ledger::CurveFinality;
use crate::shadow_ledger::{BufferedTx, MarketSnapshot, TxKey};
use anyhow::Result;
use metrics::{histogram, increment_counter};
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::warn;

#[inline]
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

const WAL_RECORD_V2_MAGIC: &[u8; 4] = b"WAL2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WalRecordClock {
    /// Explicit event/ingest provenance carried alongside the legacy WAL payload.
    #[serde(default)]
    pub event_time: EventTimeMetadata,
    /// Compatibility timestamp preserved separately from the write wall-clock.
    #[serde(default)]
    pub compat_event_ts_ms: Option<u64>,
}

impl WalRecordClock {
    pub const fn new(event_time: EventTimeMetadata, compat_event_ts_ms: Option<u64>) -> Self {
        Self {
            event_time,
            compat_event_ts_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalStorageVersion {
    LegacyV1,
    ExplicitWriteClockV2,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WalReplayEntry {
    pub record: WalRecord,
    pub write_wall_ts_ms: u64,
    pub clock: WalRecordClock,
    pub storage_version: WalStorageVersion,
}

impl WalReplayEntry {
    /// Deterministic replay ordering key that prefers the explicit write wall-clock
    /// for non-tx-path V2 records and falls back to the legacy record timestamp for V1.
    pub fn replay_order_key(&self) -> ReplayOrderKey {
        match &self.record {
            WalRecord::TradeForwarded { trade, .. } => {
                ReplayOrderKey::TxBased(trade.tx.tx_key.clone())
            }
            WalRecord::CommitStaged { commit, slot, .. } => commit
                .buffered_history
                .iter()
                .map(|tx| tx.tx_key.clone())
                .max()
                .map(ReplayOrderKey::TxBased)
                .unwrap_or(ReplayOrderKey::SlotAndWallClock {
                    slot: *slot,
                    ts_ms: self.write_wall_ts_ms,
                }),
            WalRecord::CommitPersisted { commit, slot, .. } => commit
                .last_committed_tx_key
                .clone()
                .map(ReplayOrderKey::TxBased)
                .unwrap_or(ReplayOrderKey::SlotAndWallClock {
                    slot: *slot,
                    ts_ms: self.write_wall_ts_ms,
                }),
            WalRecord::ShadowLedgerCurveUpdate { slot, .. }
            | WalRecord::RollbackReevalSeed { slot, .. } => ReplayOrderKey::SlotAndWallClock {
                slot: *slot,
                ts_ms: self.write_wall_ts_ms,
            },
            WalRecord::RawTx { .. }
            | WalRecord::ParsedEvent { .. }
            | WalRecord::Decision { .. } => ReplayOrderKey::NotRecoveryCritical,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct StoredWalRecordV2 {
    write_wall_ts_ms: u64,
    #[serde(default)]
    clock: WalRecordClock,
    record: WalRecord,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum WalRecord {
    RawTx {
        ts_ms: u64,
        slot: u64,
        signature: Option<Vec<u8>>,
        raw_tx: Vec<u8>,
    },
    ParsedEvent {
        ts_ms: u64,
        slot: u64,
        pool_id: Option<Vec<u8>>,
        kind: ParsedEventKind,
    },
    Decision {
        ts_ms: u64,
        slot: u64,
        pool_id: Option<Vec<u8>>,
        decision: GatekeeperDecision,
        reason: Option<String>,
    },
    TradeForwarded {
        ts_ms: u64,
        slot: u64,
        trade: TradeForwardRecord,
    },
    CommitStaged {
        ts_ms: u64,
        slot: u64,
        commit: CommitStagedRecord,
    },
    CommitPersisted {
        ts_ms: u64,
        slot: u64,
        commit: CommitPersistedRecord,
    },
    ShadowLedgerCurveUpdate {
        ts_ms: u64,
        slot: u64,
        update: ShadowLedgerCurveUpdateRecord,
    },
    RollbackReevalSeed {
        ts_ms: u64,
        slot: u64,
        rollback: RollbackReevalSeedRecord,
    },
}

impl WalRecord {
    /// Return the legacy timestamp field embedded in every record variant.
    ///
    /// This field is preserved for backward compatibility. New WAL segments carry
    /// an explicit write wall-clock in the V2 envelope; use `WalReplayEntry.write_wall_ts_ms`
    /// for snapshot-watermark decisions.
    #[inline]
    pub fn ts_ms(&self) -> u64 {
        match self {
            WalRecord::RawTx { ts_ms, .. }
            | WalRecord::ParsedEvent { ts_ms, .. }
            | WalRecord::Decision { ts_ms, .. }
            | WalRecord::TradeForwarded { ts_ms, .. }
            | WalRecord::CommitStaged { ts_ms, .. }
            | WalRecord::CommitPersisted { ts_ms, .. }
            | WalRecord::RollbackReevalSeed { ts_ms, .. }
            | WalRecord::ShadowLedgerCurveUpdate { ts_ms, .. } => *ts_ms,
        }
    }

    /// Return the legacy deterministic ordering key used by older WAL payloads.
    ///
    /// Recovery-critical records on the tx-path carry a `TxKey` for deterministic
    /// blockchain order. Non-tx records use `(slot, ts_ms)` as the best legacy
    /// key. New replay code should prefer `WalReplayEntry::replay_order_key()`
    /// so V2 records use the explicit write wall-clock instead of this compat field.
    pub fn replay_order_key(&self) -> ReplayOrderKey {
        match self {
            WalRecord::TradeForwarded { trade, .. } => {
                ReplayOrderKey::TxBased(trade.tx.tx_key.clone())
            }
            WalRecord::CommitStaged {
                commit,
                slot,
                ts_ms,
                ..
            } => commit
                .buffered_history
                .iter()
                .map(|tx| tx.tx_key.clone())
                .max()
                .map(ReplayOrderKey::TxBased)
                .unwrap_or(ReplayOrderKey::SlotAndWallClock {
                    slot: *slot,
                    ts_ms: *ts_ms,
                }),
            WalRecord::CommitPersisted {
                commit,
                slot,
                ts_ms,
                ..
            } => commit
                .last_committed_tx_key
                .clone()
                .map(ReplayOrderKey::TxBased)
                .unwrap_or(ReplayOrderKey::SlotAndWallClock {
                    slot: *slot,
                    ts_ms: *ts_ms,
                }),
            WalRecord::ShadowLedgerCurveUpdate { slot, ts_ms, .. } => {
                ReplayOrderKey::SlotAndWallClock {
                    slot: *slot,
                    ts_ms: *ts_ms,
                }
            }
            WalRecord::RollbackReevalSeed { slot, ts_ms, .. } => ReplayOrderKey::SlotAndWallClock {
                slot: *slot,
                ts_ms: *ts_ms,
            },
            WalRecord::RawTx { .. }
            | WalRecord::ParsedEvent { .. }
            | WalRecord::Decision { .. } => ReplayOrderKey::NotRecoveryCritical,
        }
    }
}

/// Jawny klucz porządkujący replay dla recovery-critical rekordów WAL.
///
/// Rekordy na ścieżce tx noszą `TxKey` dla deterministycznego porządku z blockchainu.
/// Rekordy poza ścieżką tx używają `(slot, ts_ms)` jako najlepszego dostępnego klucza;
/// arbitraż `write_strength` w `ShadowLedger` gwarantuje idempotencję.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReplayOrderKey {
    /// Tx-path record: deterministyczny porządek z blockchainu.
    TxBased(TxKey),
    /// Non-tx-path record: (slot, ts_ms) composite key.
    SlotAndWallClock { slot: u64, ts_ms: u64 },
    /// Nie recovery-critical (RawTx, ParsedEvent, Decision).
    NotRecoveryCritical,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ParsedEventKind {
    Create,
    Buy { lamports: u64, token_amount: u128 },
    Sell { lamports: u64, token_amount: u128 },
    Migrate,
    AccountUpdate,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum GatekeeperDecision {
    Buy,
    Reject,
    Wait,
    Timeout,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TradeForwardRecord {
    pub identity: PoolIdentity,
    pub tx: BufferedTx,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CommitStagedRecord {
    pub identity: PoolIdentity,
    pub initial_reserve_sol_lamports: u64,
    pub initial_reserve_tok_units: u64,
    pub buffered_history: Vec<BufferedTx>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CommitPersistedRecord {
    pub identity: PoolIdentity,
    pub last_committed_tx_key: Option<TxKey>,
    pub snapshots: Vec<MarketSnapshot>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RollbackReevalSeedRecord {
    pub identity: PoolIdentity,
    pub quote_mint: String,
    pub amm_program: String,
    pub creator: String,
    pub slot: Option<u64>,
    pub detected_event_ts_ms: u64,
    pub registered_wall_ts_ms: u64,
    pub initial_liquidity_sol: Option<f64>,
    pub signature: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ShadowLedgerCurveUpdateRecord {
    pub base_mint: [u8; 32],
    pub bonding_curve: [u8; 32],
    pub curve: BondingCurve,
    pub curve_data_known: bool,
    #[serde(default)]
    pub curve_finality: CurveFinality,
    pub last_update_ts_ms: u64,
    #[serde(default)]
    pub write_source: ShadowLedgerWriteSource,
    #[serde(default)]
    pub write_strength: ShadowLedgerWriteStrength,
    #[serde(default)]
    pub state_confidence: ShadowLedgerStateConfidence,
    #[serde(default)]
    pub write_reason: ShadowLedgerWriteReason,
}

struct WalSegment {
    created_at_ms: u64,
    file: File,
}

struct WalState {
    current: WalSegment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WalSyncMode {
    Buffered,
    Flush,
    #[default]
    Sync,
}

pub struct Wal {
    dir: PathBuf,
    segment_ms: u64,
    retention_ms: u64,
    sync_mode: WalSyncMode,
    state: Mutex<WalState>,
}

impl Wal {
    pub fn new<P: AsRef<Path>>(dir: P, segment_ms: u64, retention_ms: u64) -> Result<Self> {
        Self::new_with_sync_mode(dir, segment_ms, retention_ms, WalSyncMode::default())
    }

    pub fn new_with_sync_mode<P: AsRef<Path>>(
        dir: P,
        segment_ms: u64,
        retention_ms: u64,
        sync_mode: WalSyncMode,
    ) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        create_dir_all(&dir)?;
        let created_at_ms = now_ms();
        let current = open_segment(&dir, created_at_ms)?;
        Ok(Self {
            dir,
            segment_ms,
            retention_ms,
            sync_mode,
            state: Mutex::new(WalState { current }),
        })
    }

    pub fn append(&self, rec: &WalRecord) -> Result<()> {
        self.append_with_clock(rec, WalRecordClock::default())
    }

    pub fn append_with_clock(&self, rec: &WalRecord, clock: WalRecordClock) -> Result<()> {
        self.append_with_clock_at(rec, clock, now_ms())
    }

    /// Append a V2 WAL record with an explicit write wall-clock.
    ///
    /// This is primarily useful for deterministic tests and migration tooling;
    /// normal writers should prefer `append()` or `append_with_clock()`.
    pub fn append_with_clock_at(
        &self,
        rec: &WalRecord,
        clock: WalRecordClock,
        write_wall_ts_ms: u64,
    ) -> Result<()> {
        let started = Instant::now();
        let mut state = self.state.lock().expect("wal mutex poisoned");
        let now = now_ms();
        if now.saturating_sub(state.current.created_at_ms) > self.segment_ms {
            rotate_locked(&self.dir, self.retention_ms, now, &mut state)?;
        }

        let bytes = serialize_stored_record(&StoredWalRecordV2 {
            write_wall_ts_ms,
            clock,
            record: rec.clone(),
        })?;
        let len = (bytes.len() as u32).to_le_bytes();
        state.current.file.write_all(&len)?;
        state.current.file.write_all(&bytes)?;
        apply_sync_policy(&mut state.current.file, self.sync_mode)?;
        increment_counter!("wal_append_v2_records_total");
        histogram!(
            "wal_append_latency_us",
            started.elapsed().as_micros() as f64
        );
        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        let mut state = self.state.lock().expect("wal mutex poisoned");
        state.current.file.flush()?;
        state.current.file.sync_data()?;
        Ok(())
    }

    pub fn replay_all<F>(&self, mut consumer: F) -> Result<()>
    where
        F: FnMut(WalRecord),
    {
        self.replay_all_entries(|entry| consumer(entry.record))
    }

    /// Replay only the segment that may contain the watermark boundary plus any
    /// newer segments.
    ///
    /// Segment file names encode their creation timestamp, not the timestamp of
    /// every contained record. To avoid missing post-watermark records that were
    /// appended before the next rotation, recovery must include the newest
    /// segment whose creation time is **≤** the watermark and all later
    /// segments.
    pub fn replay_from_watermark<F>(&self, watermark_ms: u64, mut consumer: F) -> Result<()>
    where
        F: FnMut(WalRecord),
    {
        self.replay_from_watermark_entries(watermark_ms, |entry| consumer(entry.record))
    }

    pub fn replay_all_entries<F>(&self, mut consumer: F) -> Result<()>
    where
        F: FnMut(WalReplayEntry),
    {
        for (path, _) in replayable_segments(&self.dir, None)? {
            replay_segment_entries(&path, &mut consumer)?;
        }
        Ok(())
    }

    pub fn replay_from_watermark_entries<F>(&self, watermark_ms: u64, mut consumer: F) -> Result<()>
    where
        F: FnMut(WalReplayEntry),
    {
        for (path, _) in replayable_segments(&self.dir, Some(watermark_ms))? {
            replay_segment_entries(&path, &mut consumer)?;
        }
        Ok(())
    }
}

fn open_segment(dir: &Path, created_at_ms: u64) -> Result<WalSegment> {
    let path = dir.join(format!("segment-{}.wal", created_at_ms));
    let created = !path.exists();
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)?;
    if created {
        sync_directory(dir)?;
    }
    Ok(WalSegment {
        created_at_ms,
        file,
    })
}

fn rotate_locked(dir: &Path, retention_ms: u64, now: u64, state: &mut WalState) -> Result<()> {
    state.current.file.flush()?;
    state.current.file.sync_data()?;
    state.current = open_segment(dir, now)?;
    purge_old_segments(dir, retention_ms, now)?;
    increment_counter!("wal_segment_rotation_total");
    Ok(())
}

fn purge_old_segments(dir: &Path, retention_ms: u64, now_ms: u64) -> Result<()> {
    let mut removed_any = false;
    for (path, ts) in discover_segments(dir)? {
        if now_ms.saturating_sub(ts) > retention_ms {
            if std::fs::remove_file(path).is_ok() {
                removed_any = true;
            }
        }
    }
    if removed_any {
        sync_directory(dir)?;
    }
    Ok(())
}

fn discover_segments(dir: &Path) -> Result<Vec<(PathBuf, u64)>> {
    let mut segments = Vec::new();
    if !dir.exists() {
        return Ok(segments);
    }

    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("segment-") || !name.ends_with(".wal") {
            continue;
        }
        let Some(ts) = name
            .strip_prefix("segment-")
            .and_then(|s| s.strip_suffix(".wal"))
            .and_then(|s| s.parse::<u64>().ok())
        else {
            continue;
        };
        segments.push((path, ts));
    }

    Ok(segments)
}

fn replayable_segments(dir: &Path, watermark_ms: Option<u64>) -> Result<Vec<(PathBuf, u64)>> {
    let mut segments = discover_segments(dir)?;
    segments.sort_by_key(|(_, ts)| *ts);
    let Some(watermark_ms) = watermark_ms else {
        return Ok(segments);
    };
    if segments.is_empty() {
        return Ok(segments);
    }
    let first_newer_idx = segments.partition_point(|(_, ts)| *ts <= watermark_ms);
    let start_idx = first_newer_idx.saturating_sub(1);
    Ok(segments.into_iter().skip(start_idx).collect())
}

fn serialize_stored_record(record: &StoredWalRecordV2) -> Result<Vec<u8>> {
    let mut bytes = WAL_RECORD_V2_MAGIC.to_vec();
    bytes.extend(bincode::serialize(record)?);
    Ok(bytes)
}

fn deserialize_replay_entry(buf: &[u8]) -> Result<WalReplayEntry> {
    if let Some(payload) = buf.strip_prefix(WAL_RECORD_V2_MAGIC) {
        let stored = bincode::deserialize::<StoredWalRecordV2>(payload)?;
        return Ok(WalReplayEntry {
            record: stored.record,
            write_wall_ts_ms: stored.write_wall_ts_ms,
            clock: stored.clock,
            storage_version: WalStorageVersion::ExplicitWriteClockV2,
        });
    }

    let record = bincode::deserialize::<WalRecord>(buf)?;
    Ok(WalReplayEntry {
        write_wall_ts_ms: record.ts_ms(),
        record,
        clock: WalRecordClock::default(),
        storage_version: WalStorageVersion::LegacyV1,
    })
}

fn replay_segment_entries<F>(path: &Path, consumer: &mut F) -> Result<()>
where
    F: FnMut(WalReplayEntry),
{
    let mut file = File::open(path)?;
    loop {
        let mut len_buf = [0u8; 4];
        match file.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::UnexpectedEof => break,
            Err(err) => return Err(err.into()),
        }

        let len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        match file.read_exact(&mut buf) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::UnexpectedEof => break,
            Err(err) => return Err(err.into()),
        }

        let entry = match deserialize_replay_entry(&buf) {
            Ok(entry) => entry,
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "WAL replay: skipping incompatible record (schema mismatch)"
                );
                increment_counter!("wal_replay_schema_skip_total");
                continue;
            }
        };
        increment_counter!("wal_replay_records_total");
        match entry.storage_version {
            WalStorageVersion::LegacyV1 => increment_counter!("wal_replay_legacy_records_total"),
            WalStorageVersion::ExplicitWriteClockV2 => {
                increment_counter!("wal_replay_v2_records_total")
            }
        }
        consumer(entry);
    }
    Ok(())
}

fn apply_sync_policy(file: &mut File, sync_mode: WalSyncMode) -> Result<()> {
    match sync_mode {
        WalSyncMode::Buffered => Ok(()),
        WalSyncMode::Flush => {
            file.flush()?;
            Ok(())
        }
        WalSyncMode::Sync => {
            file.flush()?;
            file.sync_data()?;
            Ok(())
        }
    }
}

#[cfg(unix)]
fn sync_directory(dir: &Path) -> Result<()> {
    File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_dir: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn append_record_to_segment(path: &Path, record: &WalRecord) {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        let bytes = bincode::serialize(record).unwrap();
        file.write_all(&(bytes.len() as u32).to_le_bytes()).unwrap();
        file.write_all(&bytes).unwrap();
        file.flush().unwrap();
    }

    #[test]
    fn wal_append_and_replay_roundtrip() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path(), 1_000, 60_000).unwrap();

        let r1 = WalRecord::RawTx {
            ts_ms: now_ms(),
            slot: 11,
            signature: Some(vec![1, 2, 3]),
            raw_tx: vec![9, 8, 7],
        };
        let r2 = WalRecord::Decision {
            ts_ms: now_ms(),
            slot: 12,
            pool_id: Some(b"pool-a".to_vec()),
            decision: GatekeeperDecision::Buy,
            reason: Some("ok".to_string()),
        };

        wal.append(&r1).unwrap();
        wal.append(&r2).unwrap();
        wal.flush().unwrap();

        let mut out = Vec::new();
        wal.replay_all(|rec| out.push(rec)).unwrap();

        assert_eq!(out, vec![r1, r2]);
    }

    #[test]
    fn wal_replay_tolerates_truncated_tail() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path(), 1_000, 60_000).unwrap();
        let record = WalRecord::ParsedEvent {
            ts_ms: now_ms(),
            slot: 21,
            pool_id: Some(b"pool-b".to_vec()),
            kind: ParsedEventKind::Create,
        };
        wal.append(&record).unwrap();
        wal.flush().unwrap();

        let segments = discover_segments(dir.path()).unwrap();
        let (path, _) = segments.first().expect("segment should exist");
        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(&1234u32.to_le_bytes()).unwrap();
        file.write_all(&[1, 2]).unwrap();
        file.flush().unwrap();

        let mut out = Vec::new();
        wal.replay_all(|rec| out.push(rec)).unwrap();
        assert_eq!(out, vec![record]);
    }

    #[test]
    fn wal_rotation_purges_retention_expired_segments() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path(), 0, 0).unwrap();

        wal.append(&WalRecord::RawTx {
            ts_ms: now_ms(),
            slot: 1,
            signature: None,
            raw_tx: vec![1],
        })
        .unwrap();
        wal.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        wal.append(&WalRecord::RawTx {
            ts_ms: now_ms(),
            slot: 2,
            signature: None,
            raw_tx: vec![2],
        })
        .unwrap();

        let segments = discover_segments(dir.path()).unwrap();
        assert_eq!(
            segments.len(),
            1,
            "retention=0 should keep only current segment"
        );
    }

    #[test]
    fn wal_append_and_replay_10k_records() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path(), 60_000, 60_000).unwrap();
        let identity = crate::pool_identity::PoolIdentity {
            pool_id: crate::pool_identity::PoolId::from(solana_sdk::pubkey::Pubkey::new_unique()),
            base_mint: crate::pool_identity::BaseMint::from(
                solana_sdk::pubkey::Pubkey::new_unique(),
            ),
            bonding_curve: crate::pool_identity::BondingCurveKey::from(
                solana_sdk::pubkey::Pubkey::new_unique(),
            ),
        };

        for idx in 0..10_000u64 {
            wal.append(&WalRecord::TradeForwarded {
                ts_ms: idx + 1,
                slot: idx,
                trade: TradeForwardRecord {
                    identity,
                    tx: BufferedTx {
                        tx_key: TxKey::new(idx + 1, Some(idx), Some(idx as u32), None, idx + 10)
                            .unwrap(),
                        side: crate::shadow_ledger::TradeSide::Buy,
                        d_sol_lamports: 1_000_000_000 + idx,
                        d_tok_units: 1_000_000 + idx,
                        dev_buy: idx % 17 == 0,
                        trader: None,
                    },
                },
            })
            .unwrap();
        }
        wal.flush().unwrap();

        let mut replayed = 0usize;
        wal.replay_all(|_| replayed += 1).unwrap();

        assert_eq!(replayed, 10_000);
    }

    #[test]
    fn wal_replay_from_watermark_skips_fully_stale_segments() {
        let dir = tempdir().unwrap();
        let segment_1 = dir.path().join("segment-1000.wal");
        let segment_2 = dir.path().join("segment-2000.wal");
        let segment_3 = dir.path().join("segment-3000.wal");

        let r1 = WalRecord::RawTx {
            ts_ms: 1_100,
            slot: 1,
            signature: None,
            raw_tx: vec![1],
        };
        let r2 = WalRecord::RawTx {
            ts_ms: 2_100,
            slot: 2,
            signature: None,
            raw_tx: vec![2],
        };
        let r3 = WalRecord::RawTx {
            ts_ms: 3_100,
            slot: 3,
            signature: None,
            raw_tx: vec![3],
        };

        append_record_to_segment(&segment_1, &r1);
        append_record_to_segment(&segment_2, &r2);
        append_record_to_segment(&segment_3, &r3);

        let wal = Wal {
            dir: dir.path().to_path_buf(),
            segment_ms: 60_000,
            retention_ms: 60_000,
            sync_mode: WalSyncMode::Buffered,
            state: Mutex::new(WalState {
                current: open_segment(dir.path(), 3_000).unwrap(),
            }),
        };

        let mut out = Vec::new();
        wal.replay_from_watermark(2_500, |rec| out.push(rec))
            .unwrap();

        assert_eq!(
            out,
            vec![r2, r3],
            "replay must begin at the last pre-watermark segment and skip older fully stale segments"
        );
    }

    #[test]
    fn wal_replay_order_key_prefers_write_wall_clock_for_v2_non_tx_records() {
        let identity = crate::pool_identity::PoolIdentity {
            pool_id: crate::pool_identity::PoolId::from(solana_sdk::pubkey::Pubkey::new_unique()),
            base_mint: crate::pool_identity::BaseMint::from(
                solana_sdk::pubkey::Pubkey::new_unique(),
            ),
            bonding_curve: crate::pool_identity::BondingCurveKey::from(
                solana_sdk::pubkey::Pubkey::new_unique(),
            ),
        };
        let base_mint: solana_sdk::pubkey::Pubkey = identity.base_mint.into();
        let bonding_curve: solana_sdk::pubkey::Pubkey = identity.bonding_curve.into();
        let entry = WalReplayEntry {
            record: WalRecord::ShadowLedgerCurveUpdate {
                ts_ms: 1_000,
                slot: 77,
                update: ShadowLedgerCurveUpdateRecord {
                    base_mint: base_mint.to_bytes(),
                    bonding_curve: bonding_curve.to_bytes(),
                    curve: BondingCurve {
                        discriminator: 0,
                        virtual_token_reserves: 900_000_000_000,
                        virtual_sol_reserves: 31_000_000_000,
                        real_token_reserves: 800_000_000_000,
                        real_sol_reserves: 25_000_000_000,
                        token_total_supply: 1_000_000_000_000,
                        complete: 0,
                        _padding: [0; 7],
                    },
                    curve_data_known: true,
                    curve_finality: crate::shadow_ledger::CurveFinality::Provisional,
                    last_update_ts_ms: 1_000,
                    write_source: ShadowLedgerWriteSource::CompatibilityBootstrap,
                    write_strength: ShadowLedgerWriteStrength::BootstrapSeed,
                    state_confidence: ShadowLedgerStateConfidence::Speculative,
                    write_reason: ShadowLedgerWriteReason::CompatibilityBootstrap,
                },
            },
            write_wall_ts_ms: 9_000,
            clock: WalRecordClock::default(),
            storage_version: WalStorageVersion::ExplicitWriteClockV2,
        };

        assert_eq!(
            entry.replay_order_key(),
            ReplayOrderKey::SlotAndWallClock {
                slot: 77,
                ts_ms: 9_000,
            }
        );
    }

    #[test]
    fn wal_replay_order_key_keeps_legacy_payload_ts_for_v1_non_tx_records() {
        let identity = crate::pool_identity::PoolIdentity {
            pool_id: crate::pool_identity::PoolId::from(solana_sdk::pubkey::Pubkey::new_unique()),
            base_mint: crate::pool_identity::BaseMint::from(
                solana_sdk::pubkey::Pubkey::new_unique(),
            ),
            bonding_curve: crate::pool_identity::BondingCurveKey::from(
                solana_sdk::pubkey::Pubkey::new_unique(),
            ),
        };
        let entry = WalReplayEntry {
            record: WalRecord::RollbackReevalSeed {
                ts_ms: 4_200,
                slot: 88,
                rollback: RollbackReevalSeedRecord {
                    identity,
                    quote_mint: "So11111111111111111111111111111111111111112".to_string(),
                    amm_program: "pumpfun".to_string(),
                    creator: solana_sdk::pubkey::Pubkey::new_unique().to_string(),
                    slot: Some(88),
                    detected_event_ts_ms: 4_200,
                    registered_wall_ts_ms: 4_200,
                    initial_liquidity_sol: Some(25.0),
                    signature: solana_sdk::signature::Signature::new_unique().to_string(),
                    reason: "test".to_string(),
                },
            },
            write_wall_ts_ms: 4_200,
            clock: WalRecordClock::default(),
            storage_version: WalStorageVersion::LegacyV1,
        };

        assert_eq!(
            entry.replay_order_key(),
            ReplayOrderKey::SlotAndWallClock {
                slot: 88,
                ts_ms: 4_200,
            }
        );
    }
}
