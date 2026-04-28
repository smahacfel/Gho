use serde::{Deserialize, Serialize};

/// Enforces the minimal PR1/PR2 monotonic ordering for account updates.
///
/// Ordering key:
/// 1. `slot`
/// 2. `write_version.unwrap_or(u64::MAX)` as the primary same-slot tiebreaker
/// 3. `recv_seq` as the transport-local fallback tiebreaker
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonotonicUpdateGuard {
    pub last_accepted_slot: u64,
    pub last_accepted_write_version: u64,
    pub last_accepted_recv_seq: u64,
}

impl MonotonicUpdateGuard {
    #[inline]
    fn normalized_write_version(write_version: Option<u64>) -> u64 {
        write_version.unwrap_or(u64::MAX)
    }

    /// Accepts strictly newer `(slot, write_version, recv_seq)` tuples and stores
    /// them as the last accepted ordering watermark.
    pub fn accept(&mut self, slot: u64, write_version: Option<u64>, recv_seq: u64) -> bool {
        let write_version = Self::normalized_write_version(write_version);
        let should_accept = slot > self.last_accepted_slot
            || (slot == self.last_accepted_slot
                && (write_version > self.last_accepted_write_version
                    || (write_version == self.last_accepted_write_version
                        && recv_seq > self.last_accepted_recv_seq)));

        if should_accept {
            self.last_accepted_slot = slot;
            self.last_accepted_write_version = write_version;
            self.last_accepted_recv_seq = recv_seq;
        }

        should_accept
    }
}

#[cfg(test)]
mod tests {
    use super::MonotonicUpdateGuard;

    #[test]
    fn accepts_newer_slot_or_same_slot_higher_recv_seq() {
        let mut guard = MonotonicUpdateGuard::default();

        assert!(guard.accept(100, Some(1), 10));
        assert!(!guard.accept(99, Some(9), 9));
        assert!(!guard.accept(100, Some(1), 10));
        assert!(guard.accept(100, Some(2), 1));
        assert!(!guard.accept(100, Some(1), 99));
        assert!(guard.accept(100, None, 1));
        assert!(!guard.accept(100, None, 1));
        assert!(guard.accept(101, Some(0), 1));
    }
}
