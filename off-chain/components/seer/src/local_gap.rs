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
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ghost_core::{
    wal::{Wal, WalRecord, WalRecordClock},
    LocalCoverageBoundaryV1, LocalCoverageGapReasonV1, LocalCoverageGapV1,
};
use tracing::error;

const COMPLETED_GAP_CAP: usize = 1_024;
const AUDIT_QUEUE_CAP: usize = 64;
const AUDIT_PENDING_CAP: usize = 1_024;

/// Shared, bounded routing point for every local-gap domain.
///
/// Before a WAL is attached, completed markers remain in the bounded pending
/// buffer. Once installed, a dedicated reserved audit lane persists them
/// independently from the normal WAL job queue.
#[derive(Debug)]
pub(crate) struct LocalGapAuditRouter {
    sender: Mutex<Option<crossbeam_channel::Sender<LocalCoverageGapV1>>>,
    pending: Mutex<VecDeque<LocalCoverageGapV1>>,
    accepting: AtomicBool,
    overflowed: AtomicBool,
}

impl LocalGapAuditRouter {
    pub(crate) fn new() -> Self {
        Self {
            sender: Mutex::new(None),
            pending: Mutex::new(VecDeque::with_capacity(AUDIT_PENDING_CAP)),
            accepting: AtomicBool::new(true),
            overflowed: AtomicBool::new(false),
        }
    }

    pub(crate) fn emit(&self, gap: LocalCoverageGapV1) -> bool {
        if !self.accepting.load(Ordering::Acquire) {
            return false;
        }

        let sender = self
            .sender
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(sender) = sender {
            match sender.try_send(gap) {
                Ok(()) => return true,
                Err(crossbeam_channel::TrySendError::Full(gap)) => {
                    return self.retain_pending(gap);
                }
                Err(crossbeam_channel::TrySendError::Disconnected(gap)) => {
                    return self.retain_pending(gap);
                }
            }
        }

        self.retain_pending(gap)
    }

    fn retain_pending(&self, gap: LocalCoverageGapV1) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if pending.len() >= AUDIT_PENDING_CAP {
            self.overflowed.store(true, Ordering::Release);
            error!(
                gap_id = %bs58::encode(gap.gap_id_blake3).into_string(),
                "Seer: reserved local-gap audit buffer exhausted; marker retained by source tracker"
            );
            return false;
        }
        pending.push_back(gap);
        true
    }

    fn install(&self, sender: crossbeam_channel::Sender<LocalCoverageGapV1>) {
        *self.sender.lock().unwrap_or_else(|e| e.into_inner()) = Some(sender);
        self.flush_pending();
    }

    fn flush_pending(&self) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let Some(sender) = sender else {
            return;
        };

        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        while let Some(gap) = pending.pop_front() {
            match sender.try_send(gap) {
                Ok(()) => {}
                Err(crossbeam_channel::TrySendError::Full(gap))
                | Err(crossbeam_channel::TrySendError::Disconnected(gap)) => {
                    pending.push_front(gap);
                    break;
                }
            }
        }
    }

    fn stop_accepting(&self) {
        self.accepting.store(false, Ordering::Release);
        self.flush_pending();
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub(crate) fn overflowed(&self) -> bool {
        self.overflowed.load(Ordering::Acquire)
    }
}

#[derive(Clone)]
pub(crate) struct LocalGapAuditDispatcher {
    router: Arc<LocalGapAuditRouter>,
    stop: crossbeam_channel::Sender<()>,
    join: Arc<Mutex<Option<JoinHandle<()>>>>,
    append_failed: Arc<AtomicBool>,
}

impl LocalGapAuditDispatcher {
    pub(crate) fn new(wal: Arc<Wal>, router: Arc<LocalGapAuditRouter>) -> Self {
        let (sender, receiver) = crossbeam_channel::bounded(AUDIT_QUEUE_CAP);
        let (stop, stop_rx) = crossbeam_channel::bounded(1);
        let append_failed = Arc::new(AtomicBool::new(false));
        let worker_append_failed = Arc::clone(&append_failed);
        let worker_router = Arc::clone(&router);
        router.install(sender);

        let join = std::thread::Builder::new()
            .name("seer-local-gap-audit".to_string())
            .spawn(move || {
                let persist = |gap: LocalCoverageGapV1| {
                    let record = WalRecord::LocalCoverageGap {
                        ts_ms: gap.ended_at_ms,
                        slot: gap.after.slot.or(gap.before.slot).unwrap_or_default(),
                        gap,
                    };
                    match wal.append_with_clock(&record, WalRecordClock::default()) {
                        Ok(()) => {}
                        Err(err) => {
                            worker_append_failed.store(true, Ordering::Release);
                            error!(
                                error = %err,
                                "Seer: reserved local-gap audit append failed"
                            );
                        }
                    }
                };

                loop {
                    crossbeam_channel::select_biased! {
                        recv(stop_rx) -> _ => break,
                        recv(receiver) -> message => match message {
                            Ok(gap) => {
                                persist(gap);
                                worker_router.flush_pending();
                            }
                            Err(_) => break,
                        }
                    }
                }

                loop {
                    worker_router.flush_pending();
                    match receiver.try_recv() {
                        Ok(gap) => persist(gap),
                        Err(crossbeam_channel::TryRecvError::Empty)
                            if worker_router.pending_len() == 0 =>
                        {
                            break;
                        }
                        Err(crossbeam_channel::TryRecvError::Empty) => {
                            std::thread::yield_now();
                        }
                        Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                    }
                }

                if let Err(err) = wal.flush() {
                    worker_append_failed.store(true, Ordering::Release);
                    error!(error = %err, "Seer: final local-gap audit WAL flush failed");
                }
            })
            .expect("spawn reserved local-gap audit writer");

        Self {
            router,
            stop,
            join: Arc::new(Mutex::new(Some(join))),
            append_failed,
        }
    }

    pub(crate) fn shutdown_and_join(&self, timeout: Duration) -> Result<(), String> {
        self.router.stop_accepting();
        let _ = self.stop.try_send(());
        let handle = self.join.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(handle) = handle {
            let deadline = Instant::now() + timeout;
            while !handle.is_finished() {
                if Instant::now() >= deadline {
                    drop(handle);
                    return Err(format!(
                        "local-gap audit dispatcher did not drain/flush within {} ms",
                        timeout.as_millis()
                    ));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            handle
                .join()
                .map_err(|_| "local-gap audit dispatcher panicked".to_string())?;
        }
        if self.append_failed.load(Ordering::Acquire) || self.router.overflowed() {
            return Err("local-gap audit persistence failed or overflowed".to_string());
        }
        Ok(())
    }
}

#[derive(Debug)]
struct ActiveGap {
    provider_id: String,
    stream_epoch: u64,
    episode_sequence: u64,
    before: LocalCoverageBoundaryV1,
    missing_event_count: u64,
    first_dropped: LocalCoverageBoundaryV1,
    last_dropped: LocalCoverageBoundaryV1,
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
            active.missing_event_count = active.missing_event_count.saturating_add(1);
            active.last_dropped = dropped;
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
            missing_event_count: 1,
            first_dropped: dropped.clone(),
            last_dropped: dropped,
            queue_high_water,
            started_at_ms: wall_clock_ms(),
        });
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
            active.missing_event_count,
            &active.first_dropped,
            &active.last_dropped,
        );
        let gap = LocalCoverageGapV1 {
            gap_id_blake3,
            provider_id: active.provider_id,
            stream_epoch: active.stream_epoch,
            episode_sequence: active.episode_sequence,
            reason: self.reason,
            before: active.before,
            after,
            missing_event_count: active.missing_event_count,
            first_dropped: active.first_dropped,
            last_dropped: active.last_dropped,
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

    pub(crate) fn flush_completed_to(&self, router: &LocalGapAuditRouter) {
        loop {
            let Some(gap) = self.take_completed() else {
                break;
            };
            if !router.emit(gap.clone()) {
                self.state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .completed
                    .push_front(gap);
                break;
            }
        }
    }

    pub(crate) fn close_open_and_flush_to(&self, router: &LocalGapAuditRouter) {
        self.close_open_without_after();
        self.flush_completed_to(router);
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
    missing_event_count: u64,
    first_dropped: &LocalCoverageBoundaryV1,
    last_dropped: &LocalCoverageBoundaryV1,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"ghost_local_coverage_gap_v2");
    hasher.update(&(provider_id.len() as u64).to_le_bytes());
    hasher.update(provider_id.as_bytes());
    hasher.update(&stream_epoch.to_le_bytes());
    hasher.update(&episode_sequence.to_le_bytes());
    hasher.update(reason.as_str().as_bytes());
    hash_boundary(&mut hasher, before);
    hash_boundary(&mut hasher, after);
    hasher.update(&missing_event_count.to_le_bytes());
    hash_boundary(&mut hasher, first_dropped);
    hash_boundary(&mut hasher, last_dropped);
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
    use tempfile::tempdir;

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
        assert_eq!(first.missing_event_count, 2);
        assert_eq!(first.first_dropped, boundary(11, 2));
        assert_eq!(first.last_dropped, boundary(12, 3));
        assert_eq!(first.episode_sequence, 0);
        assert!(!first.recovered);
        assert!(tracker.is_unreliable());

        let replay = LocalGapTracker::new(LocalCoverageGapReasonV1::IngressQueueSaturated);
        replay.observe_admitted(boundary(10, 1));
        replay.observe_saturation("primary-a", 7, boundary(11, 2), 1_024);
        replay.observe_saturation("primary-a", 7, boundary(12, 3), 1_024);
        replay.observe_admitted(boundary(13, 4));
        assert_eq!(
            first.gap_id_blake3,
            replay.take_completed().expect("replayed gap").gap_id_blake3
        );
    }

    #[test]
    fn reserved_audit_lane_persists_every_local_gap_domain() {
        let dir = tempdir().expect("audit WAL tempdir");
        let wal = Arc::new(Wal::new(dir.path(), 60_000, 60_000).expect("audit WAL"));
        let router = Arc::new(LocalGapAuditRouter::new());
        let dispatcher = LocalGapAuditDispatcher::new(Arc::clone(&wal), Arc::clone(&router));
        let reasons = [
            LocalCoverageGapReasonV1::IngressQueueSaturated,
            LocalCoverageGapReasonV1::WalQueueSaturated,
            LocalCoverageGapReasonV1::EvidenceQueueSaturated,
            LocalCoverageGapReasonV1::IpcEgressQueueSaturated,
        ];

        for (index, reason) in reasons.into_iter().enumerate() {
            let tracker = LocalGapTracker::new(reason);
            tracker.observe_admitted(boundary(100 + index as u64 * 10, index as u8));
            tracker.observe_saturation(
                "primary-a",
                7,
                boundary(101 + index as u64 * 10, index as u8 + 10),
                64,
            );
            tracker.observe_admitted(boundary(102 + index as u64 * 10, index as u8 + 20));
            tracker.flush_completed_to(&router);
        }

        dispatcher
            .shutdown_and_join(Duration::from_secs(1))
            .expect("reserved audit lane should drain and flush");

        let mut persisted_reasons = Vec::new();
        wal.replay_all(|record| {
            if let WalRecord::LocalCoverageGap { gap, .. } = record {
                persisted_reasons.push(gap.reason);
            }
        })
        .expect("replay local-gap audit records");
        assert_eq!(persisted_reasons, reasons);
    }

    #[test]
    fn router_exposes_markers_that_cannot_be_persisted_without_an_audit_dispatcher() {
        let router = LocalGapAuditRouter::new();
        let tracker = LocalGapTracker::new(LocalCoverageGapReasonV1::IngressQueueSaturated);
        tracker.observe_admitted(boundary(100, 1));
        tracker.observe_saturation("primary-a", 7, boundary(101, 2), 64);
        tracker.observe_admitted(boundary(102, 3));
        tracker.flush_completed_to(&router);

        assert_eq!(router.pending_len(), 1);
    }
}
