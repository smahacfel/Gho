//! Cross-component append contract for the primary shadow lifecycle JSONL.
//!
//! `shadow_lifecycle.jsonl` is written by both the post-buy guardian and the
//! shadow-dispatch component.  A process-local mutex in either component is
//! insufficient: each component owns a distinct runtime object.  This helper
//! uses one advisory exclusive file lock for the complete serialized line, so
//! all cooperating writers share an ownership boundary keyed by the file.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;

use serde::Serialize;

/// Serialize and append one complete JSONL lifecycle record under the shared
/// file-level lock.
pub fn append_jsonl_record(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let mut encoded = serde_json::to_vec(value).map_err(io::Error::other)?;
    encoded.push(b'\n');
    append_prepared_jsonl_line(path, &encoded)
}

/// Append an already serialized JSONL line under the shared file-level lock.
///
/// The caller must provide exactly one JSON value followed by one newline.
/// Keeping serialization outside the lock minimizes the critical section while
/// preserving record atomicity for the actual append and flush.
pub fn append_prepared_jsonl_line(path: &Path, encoded_line: &[u8]) -> io::Result<()> {
    if encoded_line.len() < 2
        || !encoded_line.ends_with(b"\n")
        || encoded_line[..encoded_line.len() - 1].contains(&b'\n')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "lifecycle JSONL append requires exactly one non-empty line ending in a newline",
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let _lock = ExclusiveLifecycleFileLock::acquire(&file)?;
    file.write_all(encoded_line)?;
    file.flush()
}

/// RAII wrapper around the OS advisory lock. Every lifecycle writer in this
/// process uses this helper, and `flock` also covers a future cooperating
/// process writing the same inode.
struct ExclusiveLifecycleFileLock {
    #[cfg(unix)]
    fd: RawFd,
}

impl ExclusiveLifecycleFileLock {
    fn acquire(file: &File) -> io::Result<Self> {
        #[cfg(unix)]
        {
            let fd = file.as_raw_fd();
            loop {
                // SAFETY: `fd` remains valid while the caller retains `file`
                // for the lifetime of this guard. `LOCK_EX` retains no pointer
                // and does not alias Rust data.
                let result = unsafe { libc::flock(fd, libc::LOCK_EX) };
                if result == 0 {
                    return Ok(Self { fd });
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = file;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "shared lifecycle JSONL locking requires a Unix flock implementation",
            ))
        }
    }
}

impl Drop for ExclusiveLifecycleFileLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        loop {
            // SAFETY: the owner keeps `file` alive until after this guard is
            // dropped; unlocking retains no pointer and does not alias data.
            let result = unsafe { libc::flock(self.fd, libc::LOCK_UN) };
            if result == 0 || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::append_jsonl_record;
    use std::collections::BTreeSet;

    #[test]
    fn concurrent_independent_writers_preserve_jsonl_record_boundaries() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("shadow_lifecycle.jsonl");
        const WRITER_COUNT: usize = 16;
        const ROWS_PER_WRITER: usize = 32;

        std::thread::scope(|scope| {
            for writer_id in 0..WRITER_COUNT {
                let path = path.clone();
                scope.spawn(move || {
                    for row_id in 0..ROWS_PER_WRITER {
                        let record = serde_json::json!({
                            "writer_id": writer_id,
                            "row_id": row_id,
                            "payload": "x".repeat(8 * 1024),
                        });
                        append_jsonl_record(&path, &record).expect("exclusive lifecycle append");
                    }
                });
            }
        });

        let rows = std::fs::read_to_string(&path)
            .expect("read lifecycle JSONL")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid JSONL"))
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), WRITER_COUNT * ROWS_PER_WRITER);
        let identities = rows
            .iter()
            .map(|row| {
                (
                    row["writer_id"].as_u64().expect("writer id"),
                    row["row_id"].as_u64().expect("row id"),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(identities.len(), WRITER_COUNT * ROWS_PER_WRITER);
    }

    #[test]
    fn prepared_line_requires_newline() {
        let temp = tempfile::tempdir().expect("tempdir");
        let error = super::append_prepared_jsonl_line(&temp.path().join("lifecycle.jsonl"), b"{}")
            .expect_err("missing newline must fail");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn prepared_line_rejects_multiple_jsonl_rows() {
        let temp = tempfile::tempdir().expect("tempdir");
        let error =
            super::append_prepared_jsonl_line(&temp.path().join("lifecycle.jsonl"), b"{}\n{}\n")
                .expect_err("multiple JSONL rows must fail");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}
