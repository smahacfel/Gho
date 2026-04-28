use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PipelineCoverageStage {
    ChainTruth,
    GrpcReceived,
    ParsedOk,
    SeerForwarded,
    ParseMiss,
    PendingMappingBuffered,
    PendingMappingReplayed,
    SeerFiltered,
    ListenerReceived,
    ListenerForwarded,
    ListenerFiltered,
    ListenerLagged,
    SnapshotEngineReceived,
    SnapshotEngineBuffered,
    SnapshotEngineAccepted,
    SnapshotEngineFiltered,
    SnapshotEngineReplayed,
    ShadowLedgerCommitted,
    ShadowLedgerLiveAppended,
    ShadowLedgerRejected,
    /// Transactions accepted into a Gatekeeper observation buffer.
    GatekeeperBuffered,
    /// Transactions lost when a Gatekeeper buffer expires without committing.
    GatekeeperDropped,
    /// Transactions whose buffer was committed to Shadow Ledger.
    GatekeeperCommitted,
}

impl PipelineCoverageStage {
    pub const ALL: [Self; 23] = [
        Self::ChainTruth,
        Self::GrpcReceived,
        Self::ParsedOk,
        Self::SeerForwarded,
        Self::ParseMiss,
        Self::PendingMappingBuffered,
        Self::PendingMappingReplayed,
        Self::SeerFiltered,
        Self::ListenerReceived,
        Self::ListenerForwarded,
        Self::ListenerFiltered,
        Self::ListenerLagged,
        Self::SnapshotEngineReceived,
        Self::SnapshotEngineBuffered,
        Self::SnapshotEngineAccepted,
        Self::SnapshotEngineFiltered,
        Self::SnapshotEngineReplayed,
        Self::ShadowLedgerCommitted,
        Self::ShadowLedgerLiveAppended,
        Self::ShadowLedgerRejected,
        Self::GatekeeperBuffered,
        Self::GatekeeperDropped,
        Self::GatekeeperCommitted,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChainTruth => "chain_truth",
            Self::GrpcReceived => "grpc_received",
            Self::ParsedOk => "parsed_ok",
            Self::SeerForwarded => "seer_forwarded",
            Self::ParseMiss => "parse_miss",
            Self::PendingMappingBuffered => "pending_mapping_buffered",
            Self::PendingMappingReplayed => "pending_mapping_replayed",
            Self::SeerFiltered => "seer_filtered",
            Self::ListenerReceived => "listener_received",
            Self::ListenerForwarded => "listener_forwarded",
            Self::ListenerFiltered => "listener_filtered",
            Self::ListenerLagged => "listener_lagged",
            Self::SnapshotEngineReceived => "snapshot_engine_received",
            Self::SnapshotEngineBuffered => "snapshot_engine_buffered",
            Self::SnapshotEngineAccepted => "snapshot_engine_accepted",
            Self::SnapshotEngineFiltered => "snapshot_engine_filtered",
            Self::SnapshotEngineReplayed => "snapshot_engine_replayed",
            Self::ShadowLedgerCommitted => "shadow_ledger_committed",
            Self::ShadowLedgerLiveAppended => "shadow_ledger_live_appended",
            Self::ShadowLedgerRejected => "shadow_ledger_rejected",
            Self::GatekeeperBuffered => "gatekeeper_buffered",
            Self::GatekeeperDropped => "gatekeeper_dropped",
            Self::GatekeeperCommitted => "gatekeeper_committed",
        }
    }
}

#[derive(Default)]
pub struct PipelineCoverage {
    chain_truth: AtomicU64,
    grpc_received: AtomicU64,
    parsed_ok: AtomicU64,
    seer_forwarded: AtomicU64,
    parse_miss: AtomicU64,
    pending_mapping_buffered: AtomicU64,
    pending_mapping_replayed: AtomicU64,
    seer_filtered: AtomicU64,
    listener_received: AtomicU64,
    listener_forwarded: AtomicU64,
    listener_filtered: AtomicU64,
    listener_lagged: AtomicU64,
    snapshot_engine_received: AtomicU64,
    snapshot_engine_buffered: AtomicU64,
    snapshot_engine_accepted: AtomicU64,
    snapshot_engine_filtered: AtomicU64,
    snapshot_engine_replayed: AtomicU64,
    shadow_ledger_committed: AtomicU64,
    shadow_ledger_live_appended: AtomicU64,
    shadow_ledger_rejected: AtomicU64,
    gatekeeper_buffered: AtomicU64,
    gatekeeper_dropped: AtomicU64,
    gatekeeper_committed: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PipelineCoverageSnapshot {
    pub chain_truth: u64,
    pub grpc_received: u64,
    pub parsed_ok: u64,
    pub seer_forwarded: u64,
    pub parse_miss: u64,
    pub pending_mapping_buffered: u64,
    pub pending_mapping_replayed: u64,
    pub seer_filtered: u64,
    pub listener_received: u64,
    pub listener_forwarded: u64,
    pub listener_filtered: u64,
    pub listener_lagged: u64,
    pub snapshot_engine_received: u64,
    pub snapshot_engine_buffered: u64,
    pub snapshot_engine_accepted: u64,
    pub snapshot_engine_filtered: u64,
    pub snapshot_engine_replayed: u64,
    pub shadow_ledger_committed: u64,
    pub shadow_ledger_live_appended: u64,
    pub shadow_ledger_rejected: u64,
    pub gatekeeper_buffered: u64,
    pub gatekeeper_dropped: u64,
    pub gatekeeper_committed: u64,
}

impl PipelineCoverageSnapshot {
    pub fn saturating_delta_from(&self, baseline: &Self) -> Self {
        Self {
            chain_truth: self.chain_truth.saturating_sub(baseline.chain_truth),
            grpc_received: self.grpc_received.saturating_sub(baseline.grpc_received),
            parsed_ok: self.parsed_ok.saturating_sub(baseline.parsed_ok),
            seer_forwarded: self.seer_forwarded.saturating_sub(baseline.seer_forwarded),
            parse_miss: self.parse_miss.saturating_sub(baseline.parse_miss),
            pending_mapping_buffered: self
                .pending_mapping_buffered
                .saturating_sub(baseline.pending_mapping_buffered),
            pending_mapping_replayed: self
                .pending_mapping_replayed
                .saturating_sub(baseline.pending_mapping_replayed),
            seer_filtered: self.seer_filtered.saturating_sub(baseline.seer_filtered),
            listener_received: self
                .listener_received
                .saturating_sub(baseline.listener_received),
            listener_forwarded: self
                .listener_forwarded
                .saturating_sub(baseline.listener_forwarded),
            listener_filtered: self
                .listener_filtered
                .saturating_sub(baseline.listener_filtered),
            listener_lagged: self
                .listener_lagged
                .saturating_sub(baseline.listener_lagged),
            snapshot_engine_received: self
                .snapshot_engine_received
                .saturating_sub(baseline.snapshot_engine_received),
            snapshot_engine_buffered: self
                .snapshot_engine_buffered
                .saturating_sub(baseline.snapshot_engine_buffered),
            snapshot_engine_accepted: self
                .snapshot_engine_accepted
                .saturating_sub(baseline.snapshot_engine_accepted),
            snapshot_engine_filtered: self
                .snapshot_engine_filtered
                .saturating_sub(baseline.snapshot_engine_filtered),
            snapshot_engine_replayed: self
                .snapshot_engine_replayed
                .saturating_sub(baseline.snapshot_engine_replayed),
            shadow_ledger_committed: self
                .shadow_ledger_committed
                .saturating_sub(baseline.shadow_ledger_committed),
            shadow_ledger_live_appended: self
                .shadow_ledger_live_appended
                .saturating_sub(baseline.shadow_ledger_live_appended),
            shadow_ledger_rejected: self
                .shadow_ledger_rejected
                .saturating_sub(baseline.shadow_ledger_rejected),
            gatekeeper_buffered: self
                .gatekeeper_buffered
                .saturating_sub(baseline.gatekeeper_buffered),
            gatekeeper_dropped: self
                .gatekeeper_dropped
                .saturating_sub(baseline.gatekeeper_dropped),
            gatekeeper_committed: self
                .gatekeeper_committed
                .saturating_sub(baseline.gatekeeper_committed),
        }
    }

    pub fn total_for_stage(&self, stage: PipelineCoverageStage) -> u64 {
        match stage {
            PipelineCoverageStage::ChainTruth => self.chain_truth,
            PipelineCoverageStage::GrpcReceived => self.grpc_received,
            PipelineCoverageStage::ParsedOk => self.parsed_ok,
            PipelineCoverageStage::SeerForwarded => self.seer_forwarded,
            PipelineCoverageStage::ParseMiss => self.parse_miss,
            PipelineCoverageStage::PendingMappingBuffered => self.pending_mapping_buffered,
            PipelineCoverageStage::PendingMappingReplayed => self.pending_mapping_replayed,
            PipelineCoverageStage::SeerFiltered => self.seer_filtered,
            PipelineCoverageStage::ListenerReceived => self.listener_received,
            PipelineCoverageStage::ListenerForwarded => self.listener_forwarded,
            PipelineCoverageStage::ListenerFiltered => self.listener_filtered,
            PipelineCoverageStage::ListenerLagged => self.listener_lagged,
            PipelineCoverageStage::SnapshotEngineReceived => self.snapshot_engine_received,
            PipelineCoverageStage::SnapshotEngineBuffered => self.snapshot_engine_buffered,
            PipelineCoverageStage::SnapshotEngineAccepted => self.snapshot_engine_accepted,
            PipelineCoverageStage::SnapshotEngineFiltered => self.snapshot_engine_filtered,
            PipelineCoverageStage::SnapshotEngineReplayed => self.snapshot_engine_replayed,
            PipelineCoverageStage::ShadowLedgerCommitted => self.shadow_ledger_committed,
            PipelineCoverageStage::ShadowLedgerLiveAppended => self.shadow_ledger_live_appended,
            PipelineCoverageStage::ShadowLedgerRejected => self.shadow_ledger_rejected,
            PipelineCoverageStage::GatekeeperBuffered => self.gatekeeper_buffered,
            PipelineCoverageStage::GatekeeperDropped => self.gatekeeper_dropped,
            PipelineCoverageStage::GatekeeperCommitted => self.gatekeeper_committed,
        }
    }

    pub fn shadow_ledger_total(&self) -> u64 {
        self.shadow_ledger_committed
            .saturating_add(self.shadow_ledger_live_appended)
    }

    pub fn ratio_vs_chain(&self, value: u64) -> f64 {
        if self.chain_truth == 0 {
            0.0
        } else {
            (value as f64 / self.chain_truth as f64) * 100.0
        }
    }

    pub fn final_ledger_ratio(&self) -> f64 {
        self.ratio_vs_chain(self.shadow_ledger_total())
    }

    /// Fraction of Gatekeeper-buffered txs that were committed (0.0–100.0).
    ///
    /// A value below 100 % indicates txs are being dropped by Gatekeeper
    /// (observation-window timeout or veto).
    pub fn gatekeeper_commit_ratio(&self) -> f64 {
        if self.gatekeeper_buffered == 0 {
            return 0.0;
        }
        (self.gatekeeper_committed as f64 / self.gatekeeper_buffered as f64) * 100.0
    }

    /// Fraction of Gatekeeper-buffered txs that were dropped (0.0–100.0).
    pub fn gatekeeper_drop_ratio(&self) -> f64 {
        if self.gatekeeper_buffered == 0 {
            return 0.0;
        }
        (self.gatekeeper_dropped as f64 / self.gatekeeper_buffered as f64) * 100.0
    }
}

impl PipelineCoverage {
    pub fn increment(&self, stage: PipelineCoverageStage, value: u64) {
        if value == 0 {
            return;
        }

        self.counter(stage).fetch_add(value, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> PipelineCoverageSnapshot {
        PipelineCoverageSnapshot {
            chain_truth: self.chain_truth.load(Ordering::Relaxed),
            grpc_received: self.grpc_received.load(Ordering::Relaxed),
            parsed_ok: self.parsed_ok.load(Ordering::Relaxed),
            seer_forwarded: self.seer_forwarded.load(Ordering::Relaxed),
            parse_miss: self.parse_miss.load(Ordering::Relaxed),
            pending_mapping_buffered: self.pending_mapping_buffered.load(Ordering::Relaxed),
            pending_mapping_replayed: self.pending_mapping_replayed.load(Ordering::Relaxed),
            seer_filtered: self.seer_filtered.load(Ordering::Relaxed),
            listener_received: self.listener_received.load(Ordering::Relaxed),
            listener_forwarded: self.listener_forwarded.load(Ordering::Relaxed),
            listener_filtered: self.listener_filtered.load(Ordering::Relaxed),
            listener_lagged: self.listener_lagged.load(Ordering::Relaxed),
            snapshot_engine_received: self.snapshot_engine_received.load(Ordering::Relaxed),
            snapshot_engine_buffered: self.snapshot_engine_buffered.load(Ordering::Relaxed),
            snapshot_engine_accepted: self.snapshot_engine_accepted.load(Ordering::Relaxed),
            snapshot_engine_filtered: self.snapshot_engine_filtered.load(Ordering::Relaxed),
            snapshot_engine_replayed: self.snapshot_engine_replayed.load(Ordering::Relaxed),
            shadow_ledger_committed: self.shadow_ledger_committed.load(Ordering::Relaxed),
            shadow_ledger_live_appended: self.shadow_ledger_live_appended.load(Ordering::Relaxed),
            shadow_ledger_rejected: self.shadow_ledger_rejected.load(Ordering::Relaxed),
            gatekeeper_buffered: self.gatekeeper_buffered.load(Ordering::Relaxed),
            gatekeeper_dropped: self.gatekeeper_dropped.load(Ordering::Relaxed),
            gatekeeper_committed: self.gatekeeper_committed.load(Ordering::Relaxed),
        }
    }

    fn counter(&self, stage: PipelineCoverageStage) -> &AtomicU64 {
        match stage {
            PipelineCoverageStage::ChainTruth => &self.chain_truth,
            PipelineCoverageStage::GrpcReceived => &self.grpc_received,
            PipelineCoverageStage::ParsedOk => &self.parsed_ok,
            PipelineCoverageStage::SeerForwarded => &self.seer_forwarded,
            PipelineCoverageStage::ParseMiss => &self.parse_miss,
            PipelineCoverageStage::PendingMappingBuffered => &self.pending_mapping_buffered,
            PipelineCoverageStage::PendingMappingReplayed => &self.pending_mapping_replayed,
            PipelineCoverageStage::SeerFiltered => &self.seer_filtered,
            PipelineCoverageStage::ListenerReceived => &self.listener_received,
            PipelineCoverageStage::ListenerForwarded => &self.listener_forwarded,
            PipelineCoverageStage::ListenerFiltered => &self.listener_filtered,
            PipelineCoverageStage::ListenerLagged => &self.listener_lagged,
            PipelineCoverageStage::SnapshotEngineReceived => &self.snapshot_engine_received,
            PipelineCoverageStage::SnapshotEngineBuffered => &self.snapshot_engine_buffered,
            PipelineCoverageStage::SnapshotEngineAccepted => &self.snapshot_engine_accepted,
            PipelineCoverageStage::SnapshotEngineFiltered => &self.snapshot_engine_filtered,
            PipelineCoverageStage::SnapshotEngineReplayed => &self.snapshot_engine_replayed,
            PipelineCoverageStage::ShadowLedgerCommitted => &self.shadow_ledger_committed,
            PipelineCoverageStage::ShadowLedgerLiveAppended => &self.shadow_ledger_live_appended,
            PipelineCoverageStage::ShadowLedgerRejected => &self.shadow_ledger_rejected,
            PipelineCoverageStage::GatekeeperBuffered => &self.gatekeeper_buffered,
            PipelineCoverageStage::GatekeeperDropped => &self.gatekeeper_dropped,
            PipelineCoverageStage::GatekeeperCommitted => &self.gatekeeper_committed,
        }
    }
}

pub fn pipeline_coverage() -> &'static PipelineCoverage {
    static INSTANCE: OnceLock<PipelineCoverage> = OnceLock::new();
    INSTANCE.get_or_init(PipelineCoverage::default)
}

#[cfg(test)]
mod tests {
    use super::{PipelineCoverage, PipelineCoverageSnapshot, PipelineCoverageStage};

    #[test]
    fn snapshot_reports_expected_totals_and_final_ratio() {
        let coverage = PipelineCoverage::default();
        coverage.increment(PipelineCoverageStage::ChainTruth, 100);
        coverage.increment(PipelineCoverageStage::GrpcReceived, 98);
        coverage.increment(PipelineCoverageStage::ParsedOk, 97);
        coverage.increment(PipelineCoverageStage::SeerForwarded, 96);
        coverage.increment(PipelineCoverageStage::ListenerForwarded, 95);
        coverage.increment(PipelineCoverageStage::SnapshotEngineAccepted, 94);
        coverage.increment(PipelineCoverageStage::ShadowLedgerCommitted, 90);
        coverage.increment(PipelineCoverageStage::ShadowLedgerLiveAppended, 4);

        let snapshot = coverage.snapshot();
        assert_eq!(snapshot.shadow_ledger_total(), 94);
        assert_eq!(
            snapshot.total_for_stage(PipelineCoverageStage::ParsedOk),
            97
        );
        assert!((snapshot.final_ledger_ratio() - 94.0).abs() < f64::EPSILON);
    }

    #[test]
    fn zero_chain_truth_keeps_ratios_zero() {
        let snapshot = PipelineCoverageSnapshot {
            shadow_ledger_committed: 10,
            shadow_ledger_live_appended: 5,
            ..PipelineCoverageSnapshot::default()
        };

        assert_eq!(snapshot.final_ledger_ratio(), 0.0);
        assert_eq!(snapshot.ratio_vs_chain(7), 0.0);
    }

    #[test]
    fn saturating_delta_from_reports_stage_deltas() {
        let baseline = PipelineCoverageSnapshot {
            chain_truth: 10,
            grpc_received: 9,
            parsed_ok: 8,
            seer_forwarded: 7,
            shadow_ledger_committed: 6,
            shadow_ledger_live_appended: 1,
            ..PipelineCoverageSnapshot::default()
        };
        let current = PipelineCoverageSnapshot {
            chain_truth: 13,
            grpc_received: 12,
            parsed_ok: 11,
            seer_forwarded: 10,
            shadow_ledger_committed: 7,
            shadow_ledger_live_appended: 3,
            ..PipelineCoverageSnapshot::default()
        };

        let delta = current.saturating_delta_from(&baseline);
        assert_eq!(delta.chain_truth, 3);
        assert_eq!(delta.grpc_received, 3);
        assert_eq!(delta.parsed_ok, 3);
        assert_eq!(delta.seer_forwarded, 3);
        assert_eq!(delta.shadow_ledger_committed, 1);
        assert_eq!(delta.shadow_ledger_live_appended, 2);
        assert_eq!(delta.shadow_ledger_total(), 3);
        assert!((delta.final_ledger_ratio() - 100.0).abs() < f64::EPSILON);
    }
}
