//! Deterministic local saturation episodes.
//!
//! This tracker is intentionally independent from provider slot-gap handling.
//! A local queue or sink stall never asks Yellowstone to reconnect. Every
//! completed episode remains unrecovered in PR1B and therefore makes the
//! affected evaluation segment non-evaluable.

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use ghost_core::{LocalCoverageBoundaryV1, LocalCoverageGapReasonV1, LocalCoverageGapV1};

const COMPLETED_GAP_CAP: usize = 1_024;

#[derive(Debug)]
struct ActiveGap {
    provider_id: String,
    stream_epoch: u64,
    episode_sequence: u64,
    before: LocalCoverageBoundaryV1,
    queue_high_water: usize,
    started_at_ms: u64,
}

#[derive(Debug, Default)]
struct GapState {
    last_admitted: LocalCoverageBoundaryV1,
    active: Option<ActiveGap>,
    completed: VecDeque<LocalCoverageGapV1>,
}

#[derive(Debug)]
pub(crate) struct LocalGapTracker {
    reason: LocalCoverageGapReasonV1,
    sequence: AtomicU64,
    unreliable: AtomicBool,
    completed_overflow: AtomicBool,
    state: Mutex<GapState>,
}

impl LocalGapTracker {
    pub(crate) fn new(reason: LocalCoverageGapReasonV1) -> Self {
        Self {
            reason,
            sequence: AtomicU64::new(0),
            unreliable: AtomicBool::new(false),
            completed_overflow: AtomicBool::new(false),
            state: Mutex::new(GapState::default()),
        }
    }

    pub(crate) fn observe_saturation(
        &self,
        provider_id: impl Into<String>,
        stream_epoch: u64,
        dropped: LocalCoverageBoundaryV1,
        queue_high_water: usize,
    ) {
        self.unreliable.store(true, Ordering::Release);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(active) = state.active.as_mut() {
            active.queue_high_water = active.queue_high_water.max(queue_high_water);
            return;
        }

        let before = state.last_admitted.clone();
        let episode_sequence = self.sequence.fetch_add(1, Ordering::AcqRel);
        state.active = Some(ActiveGap {
            provider_id: provider_id.into(),
            stream_epoch,
            episode_sequence,
            before,
            queue_high_water,
            started_at_ms: wall_clock_ms(),
        });
        let _ = dropped;
    }

    pub(crate) fn observe_admitted(&self, after: LocalCoverageBoundaryV1) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.last_admitted = after.clone();
        let Some(active) = state.active.take() else {
            return;
        };

        let ended_at_ms = wall_clock_ms();
        let gap_id_blake3 = deterministic_gap_id(
            &active.provider_id,
            active.stream_epoch,
            active.episode_sequence,
            self.reason,
            &active.before,
            &after,
        );
        let gap = LocalCoverageGapV1 {
            gap_id_blake3,
            provider_id: active.provider_id,
            stream_epoch: active.stream_epoch,
            episode_sequence: active.episode_sequence,
            reason: self.reason,
            before: active.before,
            after,
            queue_high_water: active.queue_high_water,
            started_at_ms: active.started_at_ms,
            ended_at_ms,
            recovered: false,
        };

        if state.completed.len() >= COMPLETED_GAP_CAP {
            self.completed_overflow.store(true, Ordering::Release);
            return;
        }
        state.completed.push_back(gap);
    }

    /// Close an episode after the bounded queue has drained and no
    /// post-gap event is yet known. If a later event arrives it belongs after
    /// this already-recorded non-evaluable boundary.
    pub(crate) fn close_open_without_after(&self) {
        let last = self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .last_admitted
            .clone();
        self.observe_admitted(LocalCoverageBoundaryV1 {
            slot: None,
            signature: None,
        });
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.last_admitted = last;
    }

    pub(crate) fn take_completed(&self) -> Option<LocalCoverageGapV1> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .completed
            .pop_front()
    }

    pub(crate) fn is_unreliable(&self) -> bool {
        self.unreliable.load(Ordering::Acquire) || self.completed_overflow.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn completed_len(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .completed
            .len()
    }
}

fn wall_clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn deterministic_gap_id(
    provider_id: &str,
    stream_epoch: u64,
    episode_sequence: u64,
    reason: LocalCoverageGapReasonV1,
    before: &LocalCoverageBoundaryV1,
    after: &LocalCoverageBoundaryV1,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"ghost_local_coverage_gap_v1");
    hasher.update(&(provider_id.len() as u64).to_le_bytes());
    hasher.update(provider_id.as_bytes());
    hasher.update(&stream_epoch.to_le_bytes());
    hasher.update(&episode_sequence.to_le_bytes());
    hasher.update(reason.as_str().as_bytes());
    hash_boundary(&mut hasher, before);
    hash_boundary(&mut hasher, after);
    *hasher.finalize().as_bytes()
}

fn hash_boundary(hasher: &mut blake3::Hasher, boundary: &LocalCoverageBoundaryV1) {
    match boundary.slot {
        Some(slot) => {
            hasher.update(&[1]);
            hasher.update(&slot.to_le_bytes());
        }
        None => {
            hasher.update(&[0]);
        }
    }
    match boundary.signature {
        Some(signature) => {
            hasher.update(&[1]);
            hasher.update(signature.as_ref());
        }
        None => {
            hasher.update(&[0]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::Signature;

    fn boundary(slot: u64, seed: u8) -> LocalCoverageBoundaryV1 {
        LocalCoverageBoundaryV1 {
            slot: Some(slot),
            signature: Some(Signature::from([seed; 64])),
        }
    }

    #[test]
    fn one_continuous_saturation_episode_produces_one_deterministic_gap() {
        let tracker = LocalGapTracker::new(LocalCoverageGapReasonV1::IngressQueueSaturated);
        tracker.observe_admitted(boundary(10, 1));
        tracker.observe_saturation("primary-a", 7, boundary(11, 2), 1_024);
        tracker.observe_saturation("primary-a", 7, boundary(12, 3), 1_024);
        tracker.observe_admitted(boundary(13, 4));

        assert_eq!(tracker.completed_len(), 1);
        let first = tracker.take_completed().expect("completed gap");
        assert_eq!(first.before, boundary(10, 1));
        assert_eq!(first.after, boundary(13, 4));
        assert_eq!(first.episode_sequence, 0);
        assert!(!first.recovered);
        assert!(tracker.is_unreliable());

        let replay = LocalGapTracker::new(LocalCoverageGapReasonV1::IngressQueueSaturated);
        replay.observe_admitted(boundary(10, 1));
        replay.observe_saturation("primary-a", 7, boundary(11, 2), 1_024);
        replay.observe_admitted(boundary(13, 4));
        assert_eq!(
            first.gap_id_blake3,
            replay.take_completed().expect("replayed gap").gap_id_blake3
        );
    }
}
