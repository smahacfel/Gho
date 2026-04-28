//! Disk Snapshot Module — Periodic ShadowLedger Persistence
//!
//! Provides atomic write-to-disk and restore-from-disk operations for the full
//! ShadowLedger state (curves, aliases, snapshots, commit state, BVA archives).
//!
//! ## Design Decisions
//!
//! - **Atomic write**: data is serialized to `<name>.tmp` then renamed to `<name>` so a
//!   crash mid-write never corrupts the previous good snapshot.
//! - **Format**: bincode v1.3 (already a `ghost-core` dependency). Binary, compact, fast.
//! - **Pubkey encoding**: `[u8; 32]` (`SerPubkey`) — bincode cannot derive from the
//!   `Pubkey` tuple-struct wrapper without the `solana-sdk` serde feature, and that
//!   feature adds significant bloat; direct byte conversion is cleaner.
//! - **Rotation**: only the newest `keep_n` files are retained; filenames embed the
//!   millisecond-timestamp so sorting is lexicographic and correct.
//! - **Version guard**: field `version` must equal `SNAPSHOT_FORMAT_VERSION` (= 1) or
//!   the file is rejected.
//! - **Bootstrap filter**: snapshots with `written_at_ms == 0` are rejected (they
//!   represent uninitialized/bootstrap-only entries).

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, error, info, warn};

use super::types::{BvaArchive, SnapshotBuffer};
use super::TxKey;
use crate::market_state::ShadowBondingCurve;

// ============================================================================
// Constants
// ============================================================================

/// Current on-disk format version. Increment when the `DiskSnapshot` layout changes.
pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;

/// Prefix used for all snapshot files in the snapshot directory.
const SNAPSHOT_FILE_PREFIX: &str = "shadow_ledger_snapshot_";

/// Extension for finalized snapshot files.
const SNAPSHOT_FILE_EXT: &str = ".bin";

/// Extension for in-progress (temporary) snapshot files — never survive a crash.
const SNAPSHOT_TMP_EXT: &str = ".tmp";

// ============================================================================
// Serialization-safe alias types
// ============================================================================

/// Pubkey encoded as raw bytes for bincode serialization.
///
/// `solana_sdk::pubkey::Pubkey` is a newtype struct over `[u8; 32]`. bincode
/// serializes it as a 32-element sequence (length-prefixed), not as a fixed
/// 32-byte array. By converting to `[u8; 32]` explicitly we get a compact,
/// predictable layout that does not depend on `Pubkey`'s internal Serialize impl.
pub type SerPubkey = [u8; 32];

/// `TxKey` bytes: the full `TxKey` already derives `Serialize`/`Deserialize`, so
/// we store it as-is wrapped in `Option`.
pub type TxKeySerial = Option<TxKey>;

// ============================================================================
// Error Type
// ============================================================================

/// Errors that can occur during ShadowLedger snapshot operations.
#[derive(thiserror::Error, Debug)]
pub enum SnapshotError {
    /// No snapshot file found in the target directory.
    #[error("no snapshot found in directory: {0}")]
    NoSnapshotFound(String),

    /// The snapshot file is present but its `version` field is not `SNAPSHOT_FORMAT_VERSION`.
    #[error("unsupported snapshot version {found} (expected {expected})")]
    UnsupportedVersion { found: u32, expected: u32 },

    /// The `written_at_ms` field is zero, which indicates an uninitialized/bootstrap entry.
    #[error("snapshot rejected: written_at_ms is zero (bootstrap-only entry)")]
    ZeroTimestamp,

    /// A filesystem or IO error occurred.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization or deserialization failed.
    #[error("bincode error: {0}")]
    Bincode(#[from] Box<bincode::ErrorKind>),
}

// ============================================================================
// Stats types
// ============================================================================

/// Statistics returned by a successful `snapshot_to_disk` call.
#[derive(Debug, Clone, Copy)]
pub struct SnapshotWriteStats {
    /// Number of bonding curves written.
    pub curves_written: usize,
    /// Wall-clock time spent serializing + writing, in milliseconds.
    pub elapsed_ms: u64,
}

/// Statistics returned by a successful `restore_from_disk` call.
#[derive(Debug, Clone, Copy)]
pub struct SnapshotRestoreStats {
    /// Number of bonding curves loaded from disk.
    pub curves_loaded: usize,
    /// Wall-clock time spent reading + deserializing, in milliseconds.
    pub elapsed_ms: u64,
    /// `written_at_ms` value from the restored snapshot header.
    pub written_at_ms: u64,
}

// ============================================================================
// On-disk format
// ============================================================================

/// Complete ShadowLedger state serialized to disk.
///
/// All `DashMap` entries are flattened to `Vec` before serialization because
/// `DashMap` does not implement `Serialize`/`Deserialize`.
#[derive(Serialize, Deserialize)]
pub struct DiskSnapshot {
    /// Format version. Must equal `SNAPSHOT_FORMAT_VERSION`.
    pub version: u32,
    /// Wall-clock milliseconds when this snapshot was written.
    pub written_at_ms: u64,
    /// Convenience field: number of curves (redundant with `curves.len()`, aids debugging).
    pub curves_count: usize,
    /// Bonding curves: (bonding_curve_pubkey_bytes, ShadowBondingCurve)
    pub curves: Vec<(SerPubkey, ShadowBondingCurve)>,
    /// Alias map: (base_mint_bytes, bonding_curve_key_bytes)
    pub curve_keys_by_base_mint: Vec<(SerPubkey, SerPubkey)>,
    /// Snapshot buffers: (base_mint_bytes, SnapshotBuffer)
    pub snapshots: Vec<(SerPubkey, SnapshotBuffer)>,
    /// Commit state: (base_mint_bytes, Option<TxKey>)
    ///
    /// Stored as `Option<TxKey>` directly since `TxKey` already implements
    /// `Serialize`/`Deserialize`.
    pub snapshot_commit_state: Vec<(SerPubkey, TxKeySerial)>,
    /// BVA archives: (base_mint_bytes, BvaArchive)
    pub bva_archives: Vec<(SerPubkey, BvaArchive)>,
}

// ============================================================================
// File naming helpers
// ============================================================================

/// Build the final snapshot file path for a given timestamp.
///
/// Format: `<dir>/shadow_ledger_snapshot_<timestamp_ms>.bin`
pub fn snapshot_file_path(dir: &Path, timestamp_ms: u64) -> PathBuf {
    dir.join(format!(
        "{}{}{}",
        SNAPSHOT_FILE_PREFIX, timestamp_ms, SNAPSHOT_FILE_EXT
    ))
}

/// Build the temporary (in-progress) path for a given timestamp.
pub fn snapshot_tmp_path(dir: &Path, timestamp_ms: u64) -> PathBuf {
    dir.join(format!(
        "{}{}{}",
        SNAPSHOT_FILE_PREFIX, timestamp_ms, SNAPSHOT_TMP_EXT
    ))
}

/// List all finalized snapshot files in `dir`, sorted by embedded timestamp (ascending).
///
/// Files that do not match the expected naming pattern are silently skipped.
pub fn list_snapshot_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut entries: Vec<(u64, PathBuf)> = Vec::new();

    let read_dir_iter = match fs::read_dir(dir) {
        Ok(iter) => iter,
        // Directory does not exist yet — treat as empty (no snapshots written yet).
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    for entry in read_dir_iter {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let fname = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };
        if !fname.starts_with(SNAPSHOT_FILE_PREFIX) || !fname.ends_with(SNAPSHOT_FILE_EXT) {
            continue;
        }
        // Extract timestamp from the middle segment.
        let inner = &fname[SNAPSHOT_FILE_PREFIX.len()..fname.len() - SNAPSHOT_FILE_EXT.len()];
        if let Ok(ts) = inner.parse::<u64>() {
            entries.push((ts, path));
        }
    }

    // Sort ascending by timestamp.
    entries.sort_by_key(|(ts, _)| *ts);
    Ok(entries.into_iter().map(|(_, p)| p).collect())
}

// ============================================================================
// Core read / write functions
// ============================================================================

/// Serialize a `DiskSnapshot` atomically to `<final_path>`.
///
/// The data is first written to `<final_path>.tmp`; on success the temporary file
/// is renamed to `<final_path>`. An interrupted write therefore never clobbers an
/// existing good snapshot.
pub fn write_snapshot_atomic(
    snapshot: &DiskSnapshot,
    final_path: &Path,
) -> Result<(), SnapshotError> {
    // Derive temp path alongside the final path.
    let tmp_path = final_path.with_extension("tmp");

    // Serialize to bytes.
    let bytes = bincode::serialize(snapshot)?;

    // Write to temp file.
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }

    // Atomic rename.
    fs::rename(&tmp_path, final_path)?;

    // Sync the parent directory so the directory entry for the renamed file is
    // durably flushed to stable storage (guards against power-loss after rename
    // but before the directory write is persisted).
    if let Some(parent) = final_path.parent() {
        if let Ok(dir_file) = fs::File::open(parent) {
            let _ = dir_file.sync_all();
        }
    }

    Ok(())
}

/// Deserialize a `DiskSnapshot` from `path` and validate its header.
///
/// Returns `SnapshotError::UnsupportedVersion` or `SnapshotError::ZeroTimestamp`
/// when validation fails, leaving the file on disk unmodified.
pub fn read_snapshot(path: &Path) -> Result<DiskSnapshot, SnapshotError> {
    let bytes = fs::read(path)?;
    let snapshot: DiskSnapshot = bincode::deserialize(&bytes)?;

    if snapshot.version != SNAPSHOT_FORMAT_VERSION {
        return Err(SnapshotError::UnsupportedVersion {
            found: snapshot.version,
            expected: SNAPSHOT_FORMAT_VERSION,
        });
    }
    if snapshot.written_at_ms == 0 {
        return Err(SnapshotError::ZeroTimestamp);
    }

    Ok(snapshot)
}

/// Find and return the newest valid snapshot in `dir`.
///
/// Scans files in descending timestamp order (newest first). Returns the first
/// file that deserializes and validates successfully. Corrupted or version-mismatched
/// files are logged and skipped.
pub fn find_newest_valid_snapshot(dir: &Path) -> Result<(PathBuf, DiskSnapshot), SnapshotError> {
    let mut files = list_snapshot_files(dir)?;
    if files.is_empty() {
        return Err(SnapshotError::NoSnapshotFound(dir.display().to_string()));
    }

    // Iterate newest-first.
    files.reverse();
    for path in files {
        match read_snapshot(&path) {
            Ok(snap) => {
                debug!(
                    path = %path.display(),
                    curves = snap.curves_count,
                    written_at_ms = snap.written_at_ms,
                    "Found valid snapshot"
                );
                return Ok((path, snap));
            }
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "Skipping invalid snapshot file"
                );
            }
        }
    }

    Err(SnapshotError::NoSnapshotFound(dir.display().to_string()))
}

/// Delete old snapshot files in `dir`, retaining the `keep_n` newest.
///
/// Returns the number of files deleted.
pub fn rotate_snapshot_files(dir: &Path, keep_n: usize) -> Result<usize, SnapshotError> {
    let files = list_snapshot_files(dir)?;
    if files.len() <= keep_n {
        return Ok(0);
    }

    let to_delete = &files[..files.len() - keep_n];
    let mut deleted = 0usize;
    for path in to_delete {
        match fs::remove_file(path) {
            Ok(()) => {
                deleted += 1;
                debug!(path = %path.display(), "Deleted old snapshot file");
            }
            Err(e) => {
                error!(
                    path = %path.display(),
                    error = %e,
                    "Failed to delete old snapshot file"
                );
            }
        }
    }

    // Sync the directory so that the unlinked entries are durably recorded.
    if deleted > 0 {
        if let Ok(dir_file) = fs::File::open(dir) {
            let _ = dir_file.sync_all();
        }
    }

    Ok(deleted)
}

// ============================================================================
// High-level helpers called by ShadowLedger methods
// ============================================================================

/// Capture the current millisecond timestamp.
#[inline]
pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Write the given `DiskSnapshot` to `dir`, emitting metrics.
///
/// The file is named `shadow_ledger_snapshot_<timestamp_ms>.bin`.
/// Returns `SnapshotWriteStats` on success.
pub fn write_to_dir(
    dir: &Path,
    snapshot: DiskSnapshot,
) -> Result<SnapshotWriteStats, SnapshotError> {
    let t0 = Instant::now();
    let curves_written = snapshot.curves_count;

    // Ensure the directory exists.
    fs::create_dir_all(dir)?;

    let ts = snapshot.written_at_ms;
    let final_path = snapshot_file_path(dir, ts);

    write_snapshot_atomic(&snapshot, &final_path)?;

    let elapsed_ms = t0.elapsed().as_millis() as u64;

    info!(
        path = %final_path.display(),
        curves_written,
        elapsed_ms,
        "ShadowLedger snapshot written to disk"
    );

    metrics::gauge!(
        "shadow_ledger_snapshot_write_curves_count",
        curves_written as f64
    );
    metrics::histogram!("shadow_ledger_snapshot_write_ms", elapsed_ms as f64);

    Ok(SnapshotWriteStats {
        curves_written,
        elapsed_ms,
    })
}

/// Restore a `DiskSnapshot` from the newest valid file in `dir`.
///
/// Returns the `DiskSnapshot` plus `SnapshotRestoreStats` on success.
pub fn restore_from_dir(dir: &Path) -> Result<(DiskSnapshot, SnapshotRestoreStats), SnapshotError> {
    let t0 = Instant::now();
    let (path, snapshot) = find_newest_valid_snapshot(dir)?;

    let curves_loaded = snapshot.curves_count;
    let written_at_ms = snapshot.written_at_ms;
    let elapsed_ms = t0.elapsed().as_millis() as u64;

    info!(
        path = %path.display(),
        curves_loaded,
        written_at_ms,
        elapsed_ms,
        "ShadowLedger snapshot restored from disk"
    );

    metrics::gauge!(
        "shadow_ledger_snapshot_restore_curves_count",
        curves_loaded as f64
    );
    metrics::histogram!("shadow_ledger_snapshot_restore_ms", elapsed_ms as f64);

    Ok((
        snapshot,
        SnapshotRestoreStats {
            curves_loaded,
            elapsed_ms,
            written_at_ms,
        },
    ))
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_empty_snapshot(ts: u64) -> DiskSnapshot {
        DiskSnapshot {
            version: SNAPSHOT_FORMAT_VERSION,
            written_at_ms: ts,
            curves_count: 0,
            curves: vec![],
            curve_keys_by_base_mint: vec![],
            snapshots: vec![],
            snapshot_commit_state: vec![],
            bva_archives: vec![],
        }
    }

    #[test]
    fn test_snapshot_file_naming_roundtrip() {
        let dir = TempDir::new().unwrap();
        let ts = 1_700_000_000_000u64;
        let path = snapshot_file_path(dir.path(), ts);
        let fname = path.file_name().unwrap().to_str().unwrap();
        assert!(fname.starts_with(SNAPSHOT_FILE_PREFIX));
        assert!(fname.ends_with(SNAPSHOT_FILE_EXT));
        assert!(fname.contains(&ts.to_string()));
    }

    #[test]
    fn test_write_and_read_roundtrip() {
        let dir = TempDir::new().unwrap();
        let ts = now_ms();
        let snap = make_empty_snapshot(ts);
        let path = snapshot_file_path(dir.path(), ts);

        write_snapshot_atomic(&snap, &path).expect("write failed");
        let restored = read_snapshot(&path).expect("read failed");

        assert_eq!(restored.version, SNAPSHOT_FORMAT_VERSION);
        assert_eq!(restored.written_at_ms, ts);
        assert_eq!(restored.curves_count, 0);
    }

    #[test]
    fn test_version_mismatch_rejected() {
        let dir = TempDir::new().unwrap();
        let ts = now_ms();
        let mut snap = make_empty_snapshot(ts);
        snap.version = 99; // wrong version
        let path = snapshot_file_path(dir.path(), ts);
        write_snapshot_atomic(&snap, &path).unwrap();

        let result = read_snapshot(&path);
        assert!(matches!(
            result,
            Err(SnapshotError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn test_zero_timestamp_rejected() {
        let dir = TempDir::new().unwrap();
        let ts = now_ms();
        let mut snap = make_empty_snapshot(ts);
        snap.written_at_ms = 0; // invalid
        let path = snapshot_file_path(dir.path(), ts);
        write_snapshot_atomic(&snap, &path).unwrap();

        let result = read_snapshot(&path);
        assert!(matches!(result, Err(SnapshotError::ZeroTimestamp)));
    }

    #[test]
    fn test_list_snapshot_files_sorted() {
        let dir = TempDir::new().unwrap();
        let timestamps = [3_000u64, 1_000, 2_000];
        for &ts in &timestamps {
            let snap = make_empty_snapshot(ts);
            let path = snapshot_file_path(dir.path(), ts);
            write_snapshot_atomic(&snap, &path).unwrap();
        }

        let files = list_snapshot_files(dir.path()).unwrap();
        assert_eq!(files.len(), 3);

        // Verify ascending timestamp order by filename.
        let names: Vec<_> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        assert!(names[0].contains("1000"));
        assert!(names[1].contains("2000"));
        assert!(names[2].contains("3000"));
    }

    #[test]
    fn test_rotate_keeps_n_newest() {
        let dir = TempDir::new().unwrap();
        for ts in [1_000u64, 2_000, 3_000, 4_000, 5_000] {
            let snap = make_empty_snapshot(ts);
            let path = snapshot_file_path(dir.path(), ts);
            write_snapshot_atomic(&snap, &path).unwrap();
        }

        let deleted = rotate_snapshot_files(dir.path(), 3).unwrap();
        assert_eq!(deleted, 2, "should have deleted 2 old files");

        let remaining = list_snapshot_files(dir.path()).unwrap();
        assert_eq!(remaining.len(), 3);

        // The 3 newest timestamps should remain.
        let names: Vec<_> = remaining
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        assert!(names[0].contains("3000"));
        assert!(names[1].contains("4000"));
        assert!(names[2].contains("5000"));
    }

    #[test]
    fn test_tmp_file_not_counted_as_snapshot() {
        let dir = TempDir::new().unwrap();
        let ts = now_ms();
        // Write a .tmp file (as-if a crash happened mid-write).
        let tmp_path = snapshot_tmp_path(dir.path(), ts);
        fs::write(&tmp_path, b"garbage").unwrap();

        let files = list_snapshot_files(dir.path()).unwrap();
        assert!(
            files.is_empty(),
            ".tmp files must not appear in snapshot list"
        );
    }

    #[test]
    fn test_find_newest_skips_corrupt_and_returns_valid() {
        let dir = TempDir::new().unwrap();

        // Write a valid snapshot at ts=1000.
        let valid_snap = make_empty_snapshot(1_000);
        let valid_path = snapshot_file_path(dir.path(), 1_000);
        write_snapshot_atomic(&valid_snap, &valid_path).unwrap();

        // Write a corrupt (newer) file at ts=2000.
        let corrupt_path = snapshot_file_path(dir.path(), 2_000);
        fs::write(&corrupt_path, b"not-valid-bincode").unwrap();

        let (found_path, snap) = find_newest_valid_snapshot(dir.path()).unwrap();
        assert_eq!(found_path, valid_path);
        assert_eq!(snap.written_at_ms, 1_000);
    }

    #[test]
    fn test_no_snapshot_found_error() {
        let dir = TempDir::new().unwrap();
        let result = find_newest_valid_snapshot(dir.path());
        assert!(matches!(result, Err(SnapshotError::NoSnapshotFound(_))));
    }
}
