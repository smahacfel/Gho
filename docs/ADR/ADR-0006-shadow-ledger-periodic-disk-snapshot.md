# ADR-0006: ShadowLedger Periodic Disk Snapshot

**Date:** 2026-03-18
**Status:** Accepted
**Author:** Ghost Father

## Context

The ShadowLedger holds all bonding curve states and market snapshot buffers in memory.
Before this change, a process restart caused a total cold-start: the ledger was empty and
required re-populating from live gRPC streams, which takes time and leaves a gap in scoring
continuity. The WAL (Write-Ahead Log) provides event-level replay but reconstructing full
state from raw events on every startup is slow and not crash-proof under high traffic.

Task Z1.2 of `PLAN_INTEGRACJI_PIPELINE.md` requires a complementary persistence layer:
periodic snapshots of the full ledger state to disk that survive restarts and can be
restored quickly, after which WAL replay applies only the delta since the last snapshot.

## Decision

Implemented a self-contained `disk_snapshot` module within `ghost-core/src/shadow_ledger/`
providing:

1. **`DiskSnapshot`** — a flat, fully-owned serde struct (no `Arc`/`DashMap` inside)
   serialized with `bincode` v1.3. All `DashMap` entries are flattened to `Vec` before
   serialization. `Pubkey` values are stored as `[u8; 32]` (raw bytes) to avoid relying on
   `Pubkey`'s serde representation which is length-prefixed rather than fixed-size.

2. **Atomic write**: data goes to `<timestamp>.tmp`, then `fs::rename` atomically promotes
   it to `<timestamp>.bin`. A crash mid-write can never corrupt a previously saved snapshot.

3. **`ShadowLedger::snapshot_to_disk(dir)`** — captures full state, writes atomically.

4. **`ShadowLedger::restore_from_disk(dir)`** — scans directory, picks newest valid file
   (newest-first; skips corrupt/wrong-version files with a warning), deserializes into a
   fresh `ShadowLedger`.

5. **`ShadowLedger::rotate_snapshots(dir, keep_n)`** — deletes oldest files, keeps `keep_n`
   most recent.

6. **`main.rs` integration**:
   - `GHOST_SNAPSHOT_DIR` env var enables the feature.
   - On startup: `restore_from_disk` attempted; `NoSnapshotFound` → fresh ledger.
   - Background task: writes snapshot every `GHOST_SNAPSHOT_INTERVAL_S` seconds (default 60),
     then calls `rotate_snapshots(dir, 3)`.

7. **Metrics**: `shadow_ledger_snapshot_write_curves_count` (gauge),
   `shadow_ledger_snapshot_write_ms` (histogram), and restore equivalents.

## Architectural Impact

- `ShadowBondingCurve` in `market_state.rs`: added `#[derive(Serialize, Deserialize)]`.
  `BondingCurve` already had it. `ShadowBondingCurve` wraps `BondingCurve` and adds
  `u64`/`bool`/`u64` fields — all serde-compatible.
- `SnapshotBuffer` in `types.rs`: added `#[derive(Serialize, Deserialize)]`.
  `MarketSnapshot` and `TxKey` already had derives.
- `MintCommitState` remains private; only its `last_committed_tx_key: Option<TxKey>` field
  is serialized (restored via direct struct construction).
- New public module `disk_snapshot` exposed from `mod.rs`.
- `ShadowLedger` gains 3 new public methods: `snapshot_to_disk`, `restore_from_disk`,
  `rotate_snapshots`.
- `main.rs` gains snapshot restore on startup and a background task.

## Risk Assessment

**Low** — The snapshot mechanism is entirely additive:
- If `GHOST_SNAPSHOT_DIR` is not set, no new code paths execute in production.
- Restore failures fall back to `ShadowLedger::new()`, preserving existing cold-start
  behavior.
- Atomic write guarantees no corruption of an existing good snapshot.
- Added derives (`Serialize`, `Deserialize`) are backward-compatible (no layout change).
- `MintCommitState` is reconstructed from its serialized fields without exposing it publicly.

## Consequences

**Becomes easier:**
- Warm restart: ledger recovers its full curve + snapshot state in milliseconds.
- WAL replay only needs to process events since the last snapshot (delta replay).
- Ops tooling: snapshot files are inspectable offline with a simple bincode reader.

**Becomes harder:**
- Format migration: if `DiskSnapshot` fields change, `SNAPSHOT_FORMAT_VERSION` must be
  incremented and old files rejected. No automatic migration path.
- Disk I/O: the periodic task adds a bincode serialization + fsync every N seconds.
  At ~1000 curves with full snapshot buffers this is expected to be < 100 ms on SSD.

## Alternatives Considered

- **RocksDB / SQLite**: more powerful but adds a major dependency, increases complexity,
  and is overkill for a periodic checkpoint mechanism.
- **serde_json**: more human-readable but 5-10x larger on disk and significantly slower.
- **mmap + custom binary format**: faster but requires manual versioning/migration code
  and bytemuck constraints on all nested types including `Vec<MarketSnapshot>`.

## Validation Steps

1. `cargo build -p ghost-core` — compiles without errors. ✅
2. `cargo build -p ghost-launcher` — compiles without errors. ✅
3. `cargo test -p ghost-core shadow_ledger::disk_snapshot` — 9/9 pass. ✅
4. `cargo test -p ghost-core test_snapshot_roundtrip_1000_curves` — pass. ✅
5. `cargo test -p ghost-core test_snapshot_crash_mid_write_preserves_previous` — pass. ✅
6. `cargo test -p ghost-core test_snapshot_rotate_keeps_n_newest` — pass. ✅
7. Manual: set `GHOST_SNAPSHOT_DIR=/tmp/gh_snap GHOST_SNAPSHOT_INTERVAL_S=5`, run launcher,
   confirm `.bin` files appear in the directory after 5 s and old files are rotated.
8. Manual: stop launcher, restart with same `GHOST_SNAPSHOT_DIR`, confirm log line
   `"ShadowLedger restored from disk snapshot"` with `curves_loaded > 0`.
