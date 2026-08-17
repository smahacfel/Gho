//! IPC layer for Seer→Trigger communication
//!
//! This module provides a typed event channel with backpressure handling
//! and comprehensive metrics for monitoring the event pipeline.

use crate::types::{CandidatePool, TradeEvent};
use ghost_core::{
    CurveFinality, EventSemanticEnvelope, EventTimeMetadata, ExecutionAccountEvidence,
    ObservedPumpMutationV1, RawProviderRoleV1,
};
use prometheus::{
    register_histogram, register_int_counter, register_int_gauge, Histogram, IntCounter, IntGauge,
};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::VecDeque,
    str::FromStr,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tracing::warn;

/// Unified event type sent from Seer via IPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SeerEvent {
    /// Pool creation detected
    PoolDetected(DetectedPoolEvent),
    /// Trade (Buy/Sell) detected
    Trade(DetectedTradeEvent),
    /// Funding transfer observation forwarded from Seer ingest.
    ///
    /// The stable downstream readiness bit remains `full_chain_coverage`.
    /// Additional provenance is carried additively on the transfer payload so
    /// filtered `grpc_global_stream` observations cannot be mistaken for a
    /// future authoritative full-feed lane.
    FundingTransfer(DetectedFundingTransferEvent),
    /// On-chain AccountUpdate for a tracked pool, ready for reconciliation.
    ///
    /// Emitted every time `handle_account_update` resolves a `base_mint` and
    /// extracts valid bonding-curve reserves. The downstream reconciliation
    /// loop (OracleRuntime) consumes this to drive `process_account_update`.
    AccountUpdate(DetectedAccountUpdateEvent),
    /// Role-aware evidence for a concrete execution account.
    ///
    /// This is intentionally separate from `AccountUpdate`: it proves existence,
    /// loadability, or transport provenance for a specific account pubkey/role
    /// without mutating canonical pool reserve state.
    ExecutionAccountEvidence(DetectedExecutionAccountEvidenceEvent),
}

/// Bounded control-plane notice that an unrecovered local coverage gap was
/// observed before a normal IPC event could be delivered.  This is not a
/// business event and never enters the canonical event FIFO: using that FIFO
/// for the notice would fail precisely when saturation is the failure being
/// reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCoverageGapNoticeV1 {
    pub provider_id: String,
    pub reason: ghost_core::LocalCoverageGapReasonV1,
}

/// Bounded, monotonic control-plane retention for local coverage gaps.
///
/// A single `watch<Option<_>>` can overwrite a primary gap with a later
/// secondary notice before the launcher observes it. That is unsafe once the
/// launcher uses primary gaps as an active admission gate. The state retains
/// a bounded prefix of distinct notices instead. If the control plane itself
/// overflows, the launcher must fail closed because it can no longer prove
/// that no primary coverage gap was missed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalCoverageGapStateV1 {
    pub notices: Vec<LocalCoverageGapNoticeV1>,
    pub overflowed: bool,
}

const MAX_RETAINED_LOCAL_COVERAGE_GAP_NOTICES: usize = 64;

/// Runtime disposition attached to a raw pool-initialization observation.
///
/// `CandidateAdmission` only selects the new-candidate plane. It never grants
/// authority by itself: PR1E additionally requires a canonical Ledger permit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolDetectionRuntimeDispositionV1 {
    #[default]
    #[serde(alias = "observe")]
    CandidateAdmission,
    ContinuityOnly,
    Suppressed,
}

/// Typed pool detection event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedPoolEvent {
    /// The detected pool candidate
    pub candidate: CandidatePool,

    /// Raw Yellowstone observation aligned with `candidate`. Parsed/NLN
    /// witnesses use the same contract later but never gain canonical runtime
    /// authority through this transport field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<ObservedPumpMutationV1>,

    #[serde(default)]
    pub runtime_disposition: PoolDetectionRuntimeDispositionV1,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuity_observation_pool: Option<Pubkey>,

    /// Timestamp when the event was created (for latency tracking)
    pub detected_at: std::time::SystemTime,

    /// Event sequence number (for tracking drops)
    pub sequence_number: u64,

    /// Priority level (for future prioritization)
    pub priority: EventPriority,
}

/// Typed trade detection event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedTradeEvent {
    /// The detected trade
    pub trade: TradeEvent,

    /// Raw Yellowstone observation aligned with `trade`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<ObservedPumpMutationV1>,

    /// Timestamp when the event was created (for latency tracking)
    pub detected_at: std::time::SystemTime,

    /// Event sequence number (for tracking drops)
    pub sequence_number: u64,

    /// Priority level (for future prioritization)
    pub priority: EventPriority,
}

/// Explicit funding-transfer provenance contract carried alongside the stable
/// `full_chain_coverage` bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FundingTransferLaneKind {
    /// Current filtered Pump/PumpSwap `grpc_global_stream` lane.
    #[default]
    GrpcGlobalStreamFiltered,
    /// Dedicated filtered Pump/PumpSwap funding-only lane.
    FundingLanePumpFiltered,
    /// Future dedicated authoritative full-feed funding lane.
    AuthoritativeFullFeed,
    /// NLN Program Streams semantic transfer lane.
    NlnProgramStreams,
}

/// Coverage class for funding provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FundingTransferCoverageClass {
    /// Observation came from a filtered / partial lane and must not be used as
    /// authoritative pre-buy wallet funding coverage.
    #[default]
    FilteredObservations,
    /// Observation came from a dedicated chain-wide authoritative funding feed.
    FullChainCoverage,
}

/// Replay/audit provenance for funding transfers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FundingTransferReplayOrigin {
    /// Live ingest path.
    #[default]
    Live,
    /// Replay/backfill path.
    Replay,
}

/// Additive provenance contract for funding-transfer transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FundingTransferProvenance {
    #[serde(default)]
    pub lane_kind: FundingTransferLaneKind,
    #[serde(default)]
    pub coverage_class: FundingTransferCoverageClass,
    #[serde(default)]
    pub replay_origin: FundingTransferReplayOrigin,
}

impl Default for FundingTransferProvenance {
    fn default() -> Self {
        Self::filtered_grpc_global_stream_live()
    }
}

impl FundingTransferProvenance {
    #[must_use]
    pub const fn filtered_grpc_global_stream_live() -> Self {
        Self {
            lane_kind: FundingTransferLaneKind::GrpcGlobalStreamFiltered,
            coverage_class: FundingTransferCoverageClass::FilteredObservations,
            replay_origin: FundingTransferReplayOrigin::Live,
        }
    }

    #[must_use]
    pub const fn funding_lane_pump_filtered_live() -> Self {
        Self {
            lane_kind: FundingTransferLaneKind::FundingLanePumpFiltered,
            coverage_class: FundingTransferCoverageClass::FilteredObservations,
            replay_origin: FundingTransferReplayOrigin::Live,
        }
    }

    #[must_use]
    pub const fn authoritative_full_feed_live() -> Self {
        Self {
            lane_kind: FundingTransferLaneKind::AuthoritativeFullFeed,
            coverage_class: FundingTransferCoverageClass::FullChainCoverage,
            replay_origin: FundingTransferReplayOrigin::Live,
        }
    }

    #[must_use]
    pub const fn nln_program_streams_live(coverage_class: FundingTransferCoverageClass) -> Self {
        Self {
            lane_kind: FundingTransferLaneKind::NlnProgramStreams,
            coverage_class,
            replay_origin: FundingTransferReplayOrigin::Live,
        }
    }

    #[must_use]
    pub fn is_legacy_default(&self) -> bool {
        *self == Self::default()
    }
}

impl FundingTransferLaneKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            FundingTransferLaneKind::GrpcGlobalStreamFiltered => "grpc_global_stream_filtered",
            FundingTransferLaneKind::FundingLanePumpFiltered => "funding_lane_pump_filtered",
            FundingTransferLaneKind::AuthoritativeFullFeed => "authoritative_full_feed",
            FundingTransferLaneKind::NlnProgramStreams => "nln_program_streams",
        }
    }
}

impl FundingTransferCoverageClass {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            FundingTransferCoverageClass::FilteredObservations => "filtered_observations",
            FundingTransferCoverageClass::FullChainCoverage => "full_chain_coverage",
        }
    }
}

/// Funding transfer payload forwarded from Seer ingest into launcher IPC.
///
/// Current default producer semantics are intentionally frozen:
/// `grpc_global_stream` emits filtered observations only, so
/// `full_chain_coverage` stays `false` and the default provenance remains
/// `grpc_global_stream_filtered`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingTransferEvent {
    /// Cross-source semantic envelope carried through canonical ingest.
    #[serde(default)]
    pub semantic: EventSemanticEnvelope,

    /// Slot of the source transaction when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,

    /// Stable event ordinal within the source transaction when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_ordinal: Option<u32>,

    /// Transaction index within the Solana slot when the upstream feed exposes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_index: Option<u32>,

    /// Optional parser-side outer instruction index for execution provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_instruction_index: Option<u32>,

    /// Optional parser-side inner group index for execution provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_group_index: Option<u32>,

    /// Optional CPI stack height from the parser execution tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpi_stack_height: Option<u32>,

    /// Explicit provenance for event/ingest time axes.
    #[serde(default)]
    pub event_time: EventTimeMetadata,

    /// Monotonic arrival timestamp captured at ingest time.
    #[serde(default)]
    pub arrival_ts_ms: u64,

    /// Source transaction signature.
    pub signature: String,

    /// Funding sender wallet.
    pub source_wallet: String,

    /// Funding recipient wallet.
    pub recipient_wallet: String,

    /// Transfer size in lamports.
    pub lamports: u64,

    /// Whether the upstream feed had chain-wide coverage for wallet funding provenance.
    ///
    /// `false` means the transfer came from an opportunistic filtered lane
    /// (for example the current Pump/PumpSwap-filtered `grpc_global_stream`),
    /// so downstream FSC must not treat the stream as authoritative for
    /// pre-buy wallet funding history.
    #[serde(default)]
    pub full_chain_coverage: bool,

    /// Additive funding-lane provenance for audit, replay and future lane split.
    ///
    /// This is intentionally skipped for the current default filtered contract so
    /// legacy JSON fixtures keep their pre-PR-1 shape.
    #[serde(
        default,
        skip_serializing_if = "FundingTransferProvenance::is_legacy_default"
    )]
    pub provenance: FundingTransferProvenance,
}

/// Runtime health snapshot for the funding lane that produced an event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FundingLaneRuntimeHealth {
    /// Monotonic stream epoch, incremented when the funding lane reconnects.
    #[serde(default)]
    pub stream_epoch: u64,

    /// True when the lane has observed a reconnect/drop condition that may have
    /// created a coverage gap inside the active FSC lookback window.
    #[serde(default)]
    pub gap_suspected: bool,

    /// Cumulative dropped event count known to this producer.
    #[serde(default)]
    pub dropped_events: u64,

    /// Wall-clock timestamp of the latest known reconnect, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reconnect_ts_ms: Option<u64>,
}

impl FundingLaneRuntimeHealth {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Typed funding-transfer event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedFundingTransferEvent {
    /// The funding transfer observation.
    pub transfer: FundingTransferEvent,

    /// Runtime health of the funding lane at the time this event was emitted.
    #[serde(default, skip_serializing_if = "FundingLaneRuntimeHealth::is_default")]
    pub lane_health: FundingLaneRuntimeHealth,

    /// Timestamp when the event was created (for latency tracking).
    pub detected_at: std::time::SystemTime,

    /// Event sequence number (for tracking drops).
    pub sequence_number: u64,

    /// Priority level (for backpressure handling).
    pub priority: EventPriority,
}

/// On-chain AccountUpdate payload for reconciliation.
///
/// Carries the reserve snapshot extracted from the bonding-curve account data
/// after `base_mint` has been resolved. The values are the canonical virtual
/// reserves used by the Shadow Ledger state machine. Consumed by
/// `OracleRuntime` to drive the corrective reconciliation loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedAccountUpdateEvent {
    /// Stable raw-provider identifier when supplied by Yellowstone ingest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,

    /// Configured provider role. Metadata-only until the account arbiter lands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_role: Option<RawProviderRoleV1>,

    /// Cross-source semantic envelope carried through canonical ingest.
    #[serde(default)]
    pub semantic: EventSemanticEnvelope,

    /// Explicit provenance for event/ingest time axes.
    #[serde(default)]
    pub event_time: EventTimeMetadata,

    /// Resolved base mint (the key used by ReconciliationRuntime).
    pub base_mint: Pubkey,

    /// Bonding-curve account pubkey this update originated from.
    pub bonding_curve: Pubkey,

    /// Finality tier of the on-chain curve snapshot.
    #[serde(default)]
    pub curve_finality: CurveFinality,

    /// Virtual SOL reserves as reported on-chain.
    pub sol_reserves: u64,

    /// Virtual token reserves as reported on-chain.
    pub token_reserves: u64,

    /// Raw real SOL reserves from the canonical bonding-curve account.
    /// `None` is retained for account layouts that do not expose Pump real reserves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub real_sol_reserves: Option<u64>,

    /// Raw real token reserves from the canonical bonding-curve account.
    /// `None` is retained for account layouts that do not expose Pump real reserves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub real_token_reserves: Option<u64>,

    /// Curve completion flag (1 = graduated, 0 = active).
    pub complete: u8,

    /// Slot at which this AccountUpdate was observed.
    pub slot: u64,

    /// Optional Solana account write-version from Yellowstone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_version: Option<u64>,

    /// Signature of the transaction that produced this account write, when
    /// present in Yellowstone. Missing signatures remain `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub txn_signature: Option<solana_sdk::signature::Signature>,

    /// BLAKE3 hash of the original raw account update bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_data_hash: Option<String>,

    /// Length of the original raw account update bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_data_len: Option<u64>,

    /// Account pubkey whose raw bytes were hashed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_account_pubkey: Option<Pubkey>,

    /// Owner/program for the account bytes used as hash source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_account_owner_or_program: Option<Pubkey>,

    /// Origin of this canonical account update relative to the curve->mint mapping race window.
    #[serde(default)]
    pub replay_origin: AccountUpdateReplayOrigin,

    /// Time spent buffered before replay when the update arrived before mapping registration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_buffer_dwell_ms: Option<u64>,

    /// Wall-clock time when the event was created (for latency tracking).
    pub detected_at: std::time::SystemTime,

    /// Monotonically increasing sequence number.
    pub sequence_number: u64,
}

/// Role-aware execution account evidence payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedExecutionAccountEvidenceEvent {
    /// The structured evidence row from `ghost-core`.
    pub evidence: ExecutionAccountEvidence,

    /// Wall-clock time when the IPC event was created (for latency tracking).
    pub detected_at: std::time::SystemTime,

    /// Monotonically increasing sequence number.
    pub sequence_number: u64,

    /// Priority level (for backpressure handling).
    pub priority: EventPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccountUpdateReplayOrigin {
    #[default]
    Live,
    PendingReplay,
}

/// Event priority level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventPriority {
    /// High priority - process immediately
    High,
    /// Normal priority - standard processing
    Normal,
    /// Low priority - can be dropped under backpressure
    Low,
}

impl Default for EventPriority {
    fn default() -> Self {
        EventPriority::Normal
    }
}

/// Backpressure policy for the IPC channel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackpressurePolicy {
    /// Block sender until space is available (default)
    Block,
    /// Drop the oldest event when buffer is full
    DropOldest,
    /// Drop the current event when buffer is full
    DropNew,
    /// Drop events with Low priority first, then Normal, never High
    DropByPriority,
}

impl Default for BackpressurePolicy {
    fn default() -> Self {
        BackpressurePolicy::Block
    }
}

/// Configuration for the IPC channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcChannelConfig {
    /// Buffer size for the channel (number of events)
    pub buffer_size: usize,

    /// Backpressure policy
    pub backpressure_policy: BackpressurePolicy,

    /// Whether to log drops
    pub log_drops: bool,

    /// Whether to log overflow warnings
    pub log_overflows: bool,

    /// Warning threshold (percentage of buffer) for logging
    pub warning_threshold_percent: f64,

    /// Maximum number of canonical AccountUpdate observations that may wait in
    /// the dedicated FIFO lane.
    ///
    /// PR1B transport deliberately performs no deduplication, freshness
    /// arbitration, or latest-state coalescing. Every admitted observation is
    /// retained for the downstream PR1C account arbiter.
    #[serde(
        default = "IpcChannelConfig::default_account_update_queue_capacity",
        alias = "account_update_coalescing_capacity"
    )]
    pub account_update_queue_capacity: usize,
}

impl Default for IpcChannelConfig {
    fn default() -> Self {
        Self {
            buffer_size: 10000, // Large buffer for high-throughput bursts
            backpressure_policy: BackpressurePolicy::Block,
            log_drops: true,
            log_overflows: true,
            warning_threshold_percent: 80.0,
            account_update_queue_capacity: Self::default_account_update_queue_capacity(),
        }
    }
}

impl IpcChannelConfig {
    const fn default_account_update_queue_capacity() -> usize {
        32_768
    }
}

/// Metrics for IPC channel monitoring
#[derive(Clone)]
pub struct IpcMetrics {
    /// Total events sent through the channel
    pub events_sent: IntCounter,

    /// Total events dropped due to backpressure
    pub events_dropped: IntCounter,

    /// Total events received by consumer
    pub events_received: IntCounter,

    /// Current queue length (number of pending events)
    pub queue_length: IntGauge,

    /// Maximum queue length observed
    pub queue_length_max: IntGauge,

    /// Event handling latency (milliseconds) - from creation to consumption
    pub handling_latency_ms: Histogram,

    /// Queue wait time (milliseconds) - time spent in queue
    pub queue_wait_time_ms: Histogram,

    /// Drops by priority
    pub drops_by_priority_high: IntCounter,
    pub drops_by_priority_normal: IntCounter,
    pub drops_by_priority_low: IntCounter,
}

impl IpcMetrics {
    /// Create new IPC metrics and register them with Prometheus
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            events_sent: register_int_counter!(
                "seer_ipc_events_sent_total",
                "Total number of events sent from Seer to Trigger"
            )
            .unwrap_or_else(|_| {
                IntCounter::new(
                    "seer_ipc_events_sent_total",
                    "Total number of events sent from Seer to Trigger",
                )
                .unwrap()
            }),

            events_dropped: register_int_counter!(
                "seer_ipc_events_dropped_total",
                "Total number of events dropped due to backpressure"
            )
            .unwrap_or_else(|_| {
                IntCounter::new(
                    "seer_ipc_events_dropped_total",
                    "Total number of events dropped due to backpressure",
                )
                .unwrap()
            }),

            events_received: register_int_counter!(
                "seer_ipc_events_received_total",
                "Total number of events received by Trigger"
            )
            .unwrap_or_else(|_| {
                IntCounter::new(
                    "seer_ipc_events_received_total",
                    "Total number of events received by Trigger",
                )
                .unwrap()
            }),

            queue_length: register_int_gauge!(
                "seer_ipc_queue_length",
                "Current number of events in the IPC queue"
            )
            .unwrap_or_else(|_| {
                IntGauge::new(
                    "seer_ipc_queue_length",
                    "Current number of events in the IPC queue",
                )
                .unwrap()
            }),

            queue_length_max: register_int_gauge!(
                "seer_ipc_queue_length_max",
                "Maximum queue length observed"
            )
            .unwrap_or_else(|_| {
                IntGauge::new("seer_ipc_queue_length_max", "Maximum queue length observed").unwrap()
            }),

            handling_latency_ms: register_histogram!(
                "seer_ipc_handling_latency_ms",
                "Event handling latency from creation to consumption (milliseconds)",
                vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0]
            )
            .unwrap_or_else(|_| {
                Histogram::with_opts(
                    prometheus::HistogramOpts::new(
                        "seer_ipc_handling_latency_ms",
                        "Event handling latency from creation to consumption (milliseconds)",
                    )
                    .buckets(vec![
                        1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0,
                    ]),
                )
                .unwrap()
            }),

            queue_wait_time_ms: register_histogram!(
                "seer_ipc_queue_wait_time_ms",
                "Time events spend waiting in queue (milliseconds)",
                vec![0.1, 0.5, 1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0]
            )
            .unwrap_or_else(|_| {
                Histogram::with_opts(
                    prometheus::HistogramOpts::new(
                        "seer_ipc_queue_wait_time_ms",
                        "Time events spend waiting in queue (milliseconds)",
                    )
                    .buckets(vec![
                        0.1, 0.5, 1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0,
                    ]),
                )
                .unwrap()
            }),

            drops_by_priority_high: register_int_counter!(
                "seer_ipc_drops_by_priority_high_total",
                "Events dropped with High priority"
            )
            .unwrap_or_else(|_| {
                IntCounter::new(
                    "seer_ipc_drops_by_priority_high_total",
                    "Events dropped with High priority",
                )
                .unwrap()
            }),

            drops_by_priority_normal: register_int_counter!(
                "seer_ipc_drops_by_priority_normal_total",
                "Events dropped with Normal priority"
            )
            .unwrap_or_else(|_| {
                IntCounter::new(
                    "seer_ipc_drops_by_priority_normal_total",
                    "Events dropped with Normal priority",
                )
                .unwrap()
            }),

            drops_by_priority_low: register_int_counter!(
                "seer_ipc_drops_by_priority_low_total",
                "Events dropped with Low priority"
            )
            .unwrap_or_else(|_| {
                IntCounter::new(
                    "seer_ipc_drops_by_priority_low_total",
                    "Events dropped with Low priority",
                )
                .unwrap()
            }),
        })
    }

    /// Record an event drop
    pub fn record_drop(&self, priority: EventPriority) {
        self.events_dropped.inc();
        match priority {
            EventPriority::High => self.drops_by_priority_high.inc(),
            EventPriority::Normal => self.drops_by_priority_normal.inc(),
            EventPriority::Low => self.drops_by_priority_low.inc(),
        }
    }

    /// Update queue length metric
    pub fn update_queue_length(&self, length: usize) {
        self.queue_length.set(length as i64);
        let current_max = self.queue_length_max.get();
        if (length as i64) > current_max {
            self.queue_length_max.set(length as i64);
        }
    }

    /// Calculate drop rate as a percentage
    pub fn calculate_drop_rate(&self) -> f64 {
        let sent = self.events_sent.get() as f64;
        if sent == 0.0 {
            return 0.0;
        }
        let dropped = self.events_dropped.get() as f64;
        (dropped / sent) * 100.0
    }

    /// Get queue utilization as a percentage of capacity
    pub fn calculate_queue_utilization(&self, capacity: usize) -> f64 {
        let current = self.queue_length.get() as f64;
        (current / capacity as f64) * 100.0
    }
}

impl Default for IpcMetrics {
    fn default() -> Self {
        Self::new().as_ref().clone()
    }
}

/// Error types for IPC operations
#[derive(Debug, Error)]
pub enum IpcError {
    #[error("Channel send failed: {0}")]
    SendError(String),

    #[error("Channel receive failed")]
    ReceiveError,

    #[error("Event dropped due to backpressure (policy: {policy:?}, priority: {priority:?})")]
    EventDropped {
        policy: BackpressurePolicy,
        priority: EventPriority,
    },

    #[error("Local IPC egress coverage gap: bounded dispatcher is saturated")]
    LocalProcessingGap,

    #[error("IPC dispatcher did not drain within {timeout_ms} ms")]
    ShutdownTimeout { timeout_ms: u64 },
}

#[derive(Debug)]
enum IpcQueueError {
    Full(SeerEvent),
    Closed(SeerEvent),
}

struct IpcEgressState {
    normal: VecDeque<SeerEvent>,
    account_updates: VecDeque<SeerEvent>,
    next_sequence: u64,
    accepting: bool,
    shutdown_requested: bool,
    shutdown_deadline: Option<Instant>,
    delivery_failed: bool,
    delivery_timed_out: bool,
}

struct IpcEgressQueue {
    state: Mutex<IpcEgressState>,
    ready: Condvar,
    normal_capacity: usize,
    account_update_capacity: usize,
}

impl IpcEgressQueue {
    fn new(normal_capacity: usize, account_update_capacity: usize) -> Self {
        Self {
            state: Mutex::new(IpcEgressState {
                normal: VecDeque::with_capacity(normal_capacity),
                account_updates: VecDeque::with_capacity(account_update_capacity),
                next_sequence: 0,
                accepting: true,
                shutdown_requested: false,
                shutdown_deadline: None,
                delivery_failed: false,
                delivery_timed_out: false,
            }),
            ready: Condvar::new(),
            normal_capacity: normal_capacity.max(1),
            account_update_capacity: account_update_capacity.max(1),
        }
    }

    fn try_enqueue(&self, mut event: SeerEvent) -> Result<(), IpcQueueError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.accepting || state.delivery_failed {
            return Err(IpcQueueError::Closed(event));
        }

        if matches!(event, SeerEvent::AccountUpdate(_)) {
            if state.account_updates.len() >= self.account_update_capacity {
                return Err(IpcQueueError::Full(event));
            }
        } else if state.normal.len() >= self.normal_capacity {
            return Err(IpcQueueError::Full(event));
        }

        // Sequence allocation and queue insertion share the same lock. This is
        // the ordering linearization point for every IPC lane, so concurrent
        // producers cannot publish N+1 before N.
        let sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.saturating_add(1);
        set_seer_event_sequence(&mut event, sequence);
        if matches!(event, SeerEvent::AccountUpdate(_)) {
            state.account_updates.push_back(event);
        } else {
            state.normal.push_back(event);
        }
        self.ready.notify_one();
        Ok(())
    }

    fn len(&self) -> usize {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state
            .normal
            .len()
            .saturating_add(state.account_updates.len())
    }

    fn begin_shutdown(&self, timeout: Duration) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.accepting = false;
        state.shutdown_requested = true;
        state.shutdown_deadline = Some(Instant::now() + timeout);
        self.ready.notify_all();
    }

    fn mark_delivery_failed(&self, timed_out: bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.accepting = false;
        state.shutdown_requested = true;
        state.delivery_failed = true;
        state.delivery_timed_out |= timed_out;
        self.ready.notify_all();
    }

    fn delivery_status(&self) -> (bool, bool) {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.delivery_failed, state.delivery_timed_out)
    }

    fn shutdown_deadline_expired(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state
            .shutdown_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    fn total_capacity(&self) -> usize {
        self.normal_capacity
            .saturating_add(self.account_update_capacity)
    }

    fn next_event(&self) -> Option<SeerEvent> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let normal_sequence = state.normal.front().map(seer_event_sequence);
            let account_sequence = state.account_updates.front().map(seer_event_sequence);

            match (normal_sequence, account_sequence) {
                (Some(normal), Some(account)) if account < normal => {
                    return state.account_updates.pop_front();
                }
                (Some(_), _) => return state.normal.pop_front(),
                (None, Some(_)) => return state.account_updates.pop_front(),
                (None, None) if state.shutdown_requested => return None,
                (None, None) => {
                    state = self.ready.wait(state).unwrap_or_else(|e| e.into_inner());
                }
            }
        }
    }
}

fn seer_event_sequence(event: &SeerEvent) -> u64 {
    match event {
        SeerEvent::PoolDetected(event) => event.sequence_number,
        SeerEvent::Trade(event) => event.sequence_number,
        SeerEvent::FundingTransfer(event) => event.sequence_number,
        SeerEvent::AccountUpdate(event) => event.sequence_number,
        SeerEvent::ExecutionAccountEvidence(event) => event.sequence_number,
    }
}

fn set_seer_event_sequence(event: &mut SeerEvent, sequence: u64) {
    match event {
        SeerEvent::PoolDetected(event) => event.sequence_number = sequence,
        SeerEvent::Trade(event) => event.sequence_number = sequence,
        SeerEvent::FundingTransfer(event) => event.sequence_number = sequence,
        SeerEvent::AccountUpdate(event) => event.sequence_number = sequence,
        SeerEvent::ExecutionAccountEvidence(event) => event.sequence_number = sequence,
    }
}

/// Sender wrapper with backpressure handling and metrics
#[derive(Clone)]
pub struct IpcSender {
    /// One bounded nonblocking business-event FIFO plus one bounded canonical
    /// AccountUpdate FIFO, drained by a single fixed dispatcher.
    egress: Arc<IpcEgressQueue>,
    dispatcher: Arc<Mutex<Option<JoinHandle<()>>>>,

    /// Configuration
    config: IpcChannelConfig,

    /// Metrics
    metrics: Arc<IpcMetrics>,

    local_gap: Arc<crate::local_gap::LocalGapTracker>,
    local_gap_audit: Arc<crate::local_gap::LocalGapAuditRouter>,
    local_coverage_gap_tx: watch::Sender<LocalCoverageGapStateV1>,
}

impl IpcSender {
    /// Create a new IPC sender
    fn new(
        egress: Arc<IpcEgressQueue>,
        dispatcher: Arc<Mutex<Option<JoinHandle<()>>>>,
        config: IpcChannelConfig,
        metrics: Arc<IpcMetrics>,
        local_gap_audit: Arc<crate::local_gap::LocalGapAuditRouter>,
        local_coverage_gap_tx: watch::Sender<LocalCoverageGapStateV1>,
    ) -> Self {
        Self {
            egress,
            dispatcher,
            config,
            metrics,
            local_gap: Arc::new(crate::local_gap::LocalGapTracker::new(
                ghost_core::LocalCoverageGapReasonV1::IpcEgressQueueSaturated,
            )),
            local_gap_audit,
            local_coverage_gap_tx,
        }
    }

    pub(crate) fn local_gap_audit_router(&self) -> Arc<crate::local_gap::LocalGapAuditRouter> {
        Arc::clone(&self.local_gap_audit)
    }

    pub fn has_unrecovered_local_gap(&self) -> bool {
        self.local_gap.is_unreliable()
    }

    /// Notify the launcher control plane immediately about an unrecovered
    /// local gap. Retention is bounded and monotonic: a primary gap cannot be
    /// overwritten by a later witness notice before launcher admission sees
    /// it. If the bounded control plane itself overflows, the launcher closes
    /// admission rather than guessing which provider was affected.
    pub fn report_local_coverage_gap(
        &self,
        provider_id: impl Into<String>,
        reason: ghost_core::LocalCoverageGapReasonV1,
    ) {
        let notice = LocalCoverageGapNoticeV1 {
            provider_id: provider_id.into(),
            reason,
        };
        self.local_coverage_gap_tx.send_modify(|state| {
            if state.notices.iter().any(|existing| existing == &notice) {
                return;
            }
            if state.notices.len() >= MAX_RETAINED_LOCAL_COVERAGE_GAP_NOTICES {
                state.overflowed = true;
                return;
            }
            state.notices.push(notice);
        });
    }

    #[cfg(test)]
    pub(crate) fn dispatcher_queue_len(&self) -> usize {
        self.egress.len()
    }

    /// Send a pool detection event through the channel with backpressure handling
    pub async fn send(
        &self,
        candidate: CandidatePool,
        priority: EventPriority,
    ) -> Result<(), IpcError> {
        self.send_with_observation(candidate, None, priority).await
    }

    /// Send a pool detection together with its aligned raw observation.
    pub async fn send_with_observation(
        &self,
        candidate: CandidatePool,
        observation: Option<ObservedPumpMutationV1>,
        priority: EventPriority,
    ) -> Result<(), IpcError> {
        self.send_with_observation_and_disposition(
            candidate,
            observation,
            PoolDetectionRuntimeDispositionV1::CandidateAdmission,
            None,
            priority,
        )
        .await
    }

    pub async fn send_with_observation_and_disposition(
        &self,
        candidate: CandidatePool,
        observation: Option<ObservedPumpMutationV1>,
        runtime_disposition: PoolDetectionRuntimeDispositionV1,
        continuity_observation_pool: Option<Pubkey>,
        priority: EventPriority,
    ) -> Result<(), IpcError> {
        let event = SeerEvent::PoolDetected(DetectedPoolEvent {
            candidate,
            observation,
            runtime_disposition,
            continuity_observation_pool,
            detected_at: std::time::SystemTime::now(),
            sequence_number: 0,
            priority,
        });

        // A candidate-admission PoolDetected is structural primary evidence.
        // It must never inherit a configurable DropNew/low-priority policy:
        // saturation opens the independent local-coverage-gap control path so
        // launcher admission can fail closed. Continuity/suppressed events
        // remain non-admission traffic and retain the configured policy.
        let policy = match runtime_disposition {
            PoolDetectionRuntimeDispositionV1::CandidateAdmission => BackpressurePolicy::Block,
            PoolDetectionRuntimeDispositionV1::ContinuityOnly
            | PoolDetectionRuntimeDispositionV1::Suppressed => self.config.backpressure_policy,
        };
        self.send_event_with_policy(event, priority, policy).await
    }

    /// Send a trade event through the channel with backpressure handling
    pub async fn send_trade(
        &self,
        trade: TradeEvent,
        priority: EventPriority,
    ) -> Result<(), IpcError> {
        self.send_trade_with_observation(trade, None, priority)
            .await
    }

    /// Send a trade together with its index-aligned raw observation.
    pub async fn send_trade_with_observation(
        &self,
        trade: TradeEvent,
        observation: Option<ObservedPumpMutationV1>,
        priority: EventPriority,
    ) -> Result<(), IpcError> {
        let event = SeerEvent::Trade(DetectedTradeEvent {
            trade,
            observation,
            detected_at: std::time::SystemTime::now(),
            sequence_number: 0,
            priority,
        });

        self.send_event_with_policy(event, priority, BackpressurePolicy::Block)
            .await
    }

    /// Send a funding-transfer observation through the channel.
    ///
    /// The transport stays lossless/additive. Readiness semantics are still
    /// driven by `full_chain_coverage`, while `transfer.provenance` freezes the
    /// lane/replay contract for audit and future authoritative-lane rollout.
    pub async fn send_funding_transfer(
        &self,
        transfer: FundingTransferEvent,
        priority: EventPriority,
    ) -> Result<(), IpcError> {
        let event = SeerEvent::FundingTransfer(DetectedFundingTransferEvent {
            transfer,
            lane_health: FundingLaneRuntimeHealth::default(),
            detected_at: std::time::SystemTime::now(),
            sequence_number: 0,
            priority,
        });

        self.send_event_with_policy(event, priority, BackpressurePolicy::Block)
            .await
    }

    /// Send an AccountUpdate event for a tracked pool.
    ///
    /// AccountUpdate events drive the primary canonical-state ingest inside
    /// `OracleRuntime` / `AccountStateCore`.
    ///
    /// This is a critical path in the post-migration architecture. It never
    /// waits for downstream capacity: every admitted observation is retained
    /// in a bounded FIFO. PR1B does not compare slot/write-version, hash,
    /// provider, signature, or reserve content; that arbitration belongs to
    /// PR1C.
    ///
    /// `sol_reserves` / `token_reserves` must be the canonical virtual reserves
    /// from the bonding-curve account, not the real balance subset.
    pub async fn send_account_update(
        &self,
        provider_id: Option<String>,
        provider_role: Option<RawProviderRoleV1>,
        semantic: EventSemanticEnvelope,
        event_time: EventTimeMetadata,
        base_mint: Pubkey,
        bonding_curve: Pubkey,
        curve_finality: CurveFinality,
        sol_reserves: u64,
        token_reserves: u64,
        real_sol_reserves: Option<u64>,
        real_token_reserves: Option<u64>,
        complete: u8,
        slot: u64,
        write_version: Option<u64>,
        txn_signature: Option<solana_sdk::signature::Signature>,
        account_data_hash: Option<String>,
        account_data_len: Option<u64>,
        source_account_pubkey: Option<Pubkey>,
        source_account_owner_or_program: Option<Pubkey>,
        replay_origin: AccountUpdateReplayOrigin,
        replay_buffer_dwell_ms: Option<u64>,
    ) -> Result<(), IpcError> {
        let event = SeerEvent::AccountUpdate(DetectedAccountUpdateEvent {
            provider_id,
            provider_role,
            semantic,
            event_time,
            base_mint,
            bonding_curve,
            curve_finality,
            sol_reserves,
            token_reserves,
            real_sol_reserves,
            real_token_reserves,
            complete,
            slot,
            write_version,
            txn_signature,
            account_data_hash,
            account_data_len,
            source_account_pubkey,
            source_account_owner_or_program,
            replay_origin,
            replay_buffer_dwell_ms,
            detected_at: std::time::SystemTime::now(),
            sequence_number: 0,
        });

        self.send_event_with_policy(event, EventPriority::High, BackpressurePolicy::Block)
            .await
    }

    /// Send role-aware execution account evidence through the IPC channel.
    ///
    /// Evidence is a separate transport contract from canonical reserve
    /// `AccountUpdate` events and must not be routed through that path.
    pub async fn send_execution_account_evidence(
        &self,
        evidence: ExecutionAccountEvidence,
        priority: EventPriority,
    ) -> Result<(), IpcError> {
        let event = SeerEvent::ExecutionAccountEvidence(DetectedExecutionAccountEvidenceEvent {
            evidence,
            detected_at: std::time::SystemTime::now(),
            sequence_number: 0,
            priority,
        });

        self.send_event_with_policy(event, priority, BackpressurePolicy::Block)
            .await
    }

    #[must_use]
    pub fn current_queue_length(&self) -> usize {
        self.egress.len()
    }

    async fn send_event_with_policy(
        &self,
        event: SeerEvent,
        priority: EventPriority,
        policy: BackpressurePolicy,
    ) -> Result<(), IpcError> {
        // Measure the single bounded egress-dispatch queue.
        let current_queue_length = self.egress.len();
        self.metrics.update_queue_length(current_queue_length);

        // Check if we're approaching capacity
        let utilization = self
            .metrics
            .calculate_queue_utilization(self.egress.total_capacity());
        if utilization >= self.config.warning_threshold_percent && self.config.log_overflows {
            warn!(
                "IPC queue utilization high: {:.1}% ({}/{})",
                utilization,
                current_queue_length,
                self.egress.total_capacity()
            );
        }

        let boundary = ipc_event_boundary(&event);
        let provider_id = ipc_event_provider_id(&event);
        let send_result = match self.egress.try_enqueue(event) {
            Ok(()) => {
                self.local_gap.observe_admitted(boundary);
                self.local_gap.flush_completed_to(&self.local_gap_audit);
                Ok(())
            }
            Err(IpcQueueError::Full(_event)) => {
                if matches!(policy, BackpressurePolicy::DropNew)
                    || matches!(policy, BackpressurePolicy::DropByPriority)
                        && priority == EventPriority::Low
                {
                    self.metrics.record_drop(priority);
                    Err(IpcError::EventDropped { policy, priority })
                } else {
                    self.local_gap.observe_saturation(
                        provider_id.clone(),
                        0,
                        boundary,
                        current_queue_length.max(self.config.buffer_size),
                    );
                    ::metrics::increment_counter!(
                        "seer_local_coverage_gap_opened_total",
                        "reason" => "ipc_egress_queue_saturated"
                    );
                    self.report_local_coverage_gap(
                        provider_id,
                        ghost_core::LocalCoverageGapReasonV1::IpcEgressQueueSaturated,
                    );
                    Err(IpcError::LocalProcessingGap)
                }
            }
            Err(IpcQueueError::Closed(_event)) => Err(IpcError::SendError(
                "IPC egress dispatcher disconnected".to_string(),
            )),
        };

        match send_result {
            Ok(_) => {
                self.metrics.events_sent.inc();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Get current queue utilization
    pub fn queue_utilization(&self) -> f64 {
        self.metrics.update_queue_length(self.egress.len());
        self.metrics
            .calculate_queue_utilization(self.egress.total_capacity())
    }

    /// Get drop rate
    pub fn drop_rate(&self) -> f64 {
        self.metrics.calculate_drop_rate()
    }

    /// Stop accepting new events, drain every accepted business/state event to
    /// the downstream receiver, and join the fixed dispatcher thread.
    pub fn shutdown_and_join(&self, timeout: Duration) -> Result<(), IpcError> {
        self.egress.begin_shutdown(timeout);
        let deadline = Instant::now() + timeout;
        loop {
            let finished = self
                .dispatcher
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .is_none_or(JoinHandle::is_finished);
            if finished {
                break;
            }
            if Instant::now() >= deadline {
                self.egress.mark_delivery_failed(true);
                // Dropping a still-running handle detaches the fixed worker;
                // it observes the same deadline and exits without a blocking send.
                self.dispatcher
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take();
                self.local_gap
                    .close_open_and_flush_to(&self.local_gap_audit);
                return Err(IpcError::ShutdownTimeout {
                    timeout_ms: timeout.as_millis().min(u128::from(u64::MAX)) as u64,
                });
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if let Some(handle) = self
            .dispatcher
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            handle
                .join()
                .map_err(|_| IpcError::SendError("IPC egress dispatcher panicked".to_string()))?;
        }
        self.local_gap
            .close_open_and_flush_to(&self.local_gap_audit);
        let (delivery_failed, delivery_timed_out) = self.egress.delivery_status();
        if delivery_timed_out {
            return Err(IpcError::ShutdownTimeout {
                timeout_ms: timeout.as_millis().min(u128::from(u64::MAX)) as u64,
            });
        }
        if delivery_failed {
            return Err(IpcError::SendError(
                "IPC downstream closed before all accepted events were delivered".to_string(),
            ));
        }
        Ok(())
    }
}

/// Receiver wrapper with metrics tracking
pub struct IpcReceiver {
    /// Underlying channel receiver
    receiver: mpsc::Receiver<SeerEvent>,

    /// Metrics
    metrics: Arc<IpcMetrics>,
    local_coverage_gap_rx: watch::Receiver<LocalCoverageGapStateV1>,
}

/// Extract the `detected_at` timestamp from any `SeerEvent` variant.
fn event_detected_at(event: &SeerEvent) -> &std::time::SystemTime {
    match event {
        SeerEvent::PoolDetected(e) => &e.detected_at,
        SeerEvent::Trade(e) => &e.detected_at,
        SeerEvent::FundingTransfer(e) => &e.detected_at,
        SeerEvent::AccountUpdate(e) => &e.detected_at,
        SeerEvent::ExecutionAccountEvidence(e) => &e.detected_at,
    }
}

fn ipc_event_provider_id(event: &SeerEvent) -> String {
    match event {
        SeerEvent::PoolDetected(event) => event
            .candidate
            .provider_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        SeerEvent::Trade(event) => event
            .trade
            .provider_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        SeerEvent::AccountUpdate(event) => event
            .provider_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        _ => "seer".to_string(),
    }
}

fn ipc_event_boundary(event: &SeerEvent) -> ghost_core::LocalCoverageBoundaryV1 {
    match event {
        SeerEvent::PoolDetected(event) => ghost_core::LocalCoverageBoundaryV1 {
            slot: event.candidate.slot,
            signature: solana_sdk::signature::Signature::from_str(&event.candidate.signature).ok(),
        },
        SeerEvent::Trade(event) => ghost_core::LocalCoverageBoundaryV1 {
            slot: event.trade.slot,
            signature: Some(event.trade.signature),
        },
        SeerEvent::AccountUpdate(event) => ghost_core::LocalCoverageBoundaryV1 {
            slot: Some(event.slot),
            signature: event.txn_signature,
        },
        _ => ghost_core::LocalCoverageBoundaryV1::default(),
    }
}

impl IpcReceiver {
    /// Create a new IPC receiver
    pub fn new(
        receiver: mpsc::Receiver<SeerEvent>,
        metrics: Arc<IpcMetrics>,
        local_coverage_gap_rx: watch::Receiver<LocalCoverageGapStateV1>,
    ) -> Self {
        Self {
            receiver,
            metrics,
            local_coverage_gap_rx,
        }
    }

    /// Clone the overwrite-only local-gap control signal. The launcher uses
    /// it independently of the business FIFO so an IPC saturation cannot be
    /// hidden by a subsequent lack of normal events.
    #[must_use]
    pub fn local_coverage_gap_receiver(&self) -> watch::Receiver<LocalCoverageGapStateV1> {
        self.local_coverage_gap_rx.clone()
    }

    /// Record handling latency for the given event using the shared helper.
    fn record_latency(&self, event: &SeerEvent) {
        if let Ok(duration) = event_detected_at(event).elapsed() {
            self.metrics
                .handling_latency_ms
                .observe(duration.as_secs_f64() * 1000.0);
        }
    }

    /// Receive an event from the channel
    pub async fn recv(&mut self) -> Option<SeerEvent> {
        let event = self.receiver.recv().await?;

        self.metrics.events_received.inc();
        self.record_latency(&event);

        Some(event)
    }

    /// Try to receive an event without blocking
    pub fn try_recv(&mut self) -> Result<SeerEvent, mpsc::error::TryRecvError> {
        let event = self.receiver.try_recv()?;

        self.metrics.events_received.inc();
        self.record_latency(&event);

        Ok(event)
    }
}

/// Create a new IPC channel with the given configuration
pub fn create_ipc_channel(config: IpcChannelConfig) -> (IpcSender, IpcReceiver, Arc<IpcMetrics>) {
    let (downstream_tx, rx) = mpsc::channel(config.buffer_size);
    let metrics = IpcMetrics::new();
    let local_gap_audit = Arc::new(crate::local_gap::LocalGapAuditRouter::new());
    let (local_coverage_gap_tx, local_coverage_gap_rx) =
        watch::channel(LocalCoverageGapStateV1::default());
    let egress = Arc::new(IpcEgressQueue::new(
        config.buffer_size,
        config.account_update_queue_capacity,
    ));
    let worker_egress = Arc::clone(&egress);

    let handle = std::thread::Builder::new()
        .name("seer-ipc-egress".to_string())
        .spawn(move || {
            let mut pending = None;
            loop {
                let event = match pending.take().or_else(|| worker_egress.next_event()) {
                    Some(event) => event,
                    None => break,
                };
                match downstream_tx.try_send(event) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(event)) => {
                        pending = Some(event);
                        if worker_egress.shutdown_deadline_expired() {
                            worker_egress.mark_delivery_failed(true);
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(mpsc::error::TrySendError::Closed(_event)) => {
                        worker_egress.mark_delivery_failed(false);
                        break;
                    }
                }
            }
        })
        .expect("spawn fixed Seer IPC egress dispatcher");
    let dispatcher = Arc::new(Mutex::new(Some(handle)));

    let sender = IpcSender::new(
        egress,
        dispatcher,
        config.clone(),
        Arc::clone(&metrics),
        local_gap_audit,
        local_coverage_gap_tx,
    );
    let receiver = IpcReceiver::new(rx, Arc::clone(&metrics), local_coverage_gap_rx);

    (sender, receiver, metrics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CandidatePool;
    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn old_ipc_config_defaults_account_update_queue_capacity() {
        let mut value =
            serde_json::to_value(IpcChannelConfig::default()).expect("serialize IPC config");
        value
            .as_object_mut()
            .expect("IPC config object")
            .remove("account_update_queue_capacity");
        let decoded: IpcChannelConfig =
            serde_json::from_value(value).expect("deserialize old IPC config");
        assert_eq!(
            decoded.account_update_queue_capacity,
            IpcChannelConfig::default().account_update_queue_capacity
        );
    }

    #[test]
    fn legacy_coalescing_capacity_name_is_a_deserialize_only_alias() {
        let defaults = IpcChannelConfig::default();
        let value = serde_json::json!({
            "buffer_size": defaults.buffer_size,
            "backpressure_policy": defaults.backpressure_policy,
            "log_drops": defaults.log_drops,
            "log_overflows": defaults.log_overflows,
            "warning_threshold_percent": defaults.warning_threshold_percent,
            "account_update_coalescing_capacity": 123
        });
        let decoded: IpcChannelConfig =
            serde_json::from_value(value).expect("deserialize legacy IPC config");
        assert_eq!(decoded.account_update_queue_capacity, 123);
    }

    async fn wait_for_downstream_len(receiver: &IpcReceiver, expected: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if receiver.receiver.len() == expected {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("fixed IPC dispatcher should reach expected downstream occupancy");
    }

    async fn wait_for_dispatcher_len(sender: &IpcSender, expected: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if sender.dispatcher_queue_len() == expected {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("fixed IPC dispatcher queue should reach expected occupancy");
    }

    fn create_test_candidate() -> CandidatePool {
        CandidatePool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            provider_id: None,
            provider_role: None,
            slot: Some(100),
            tx_index: None,
            birth_canonical_order: None,
            event_ts_ms: Some(1_234_567_890_000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: "test_sig".to_string(),
            amm_program_id: Pubkey::new_unique(),
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            creation_regime: ghost_core::PumpCreationRegimeV1::default(),
            bonding_curve: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            timestamp: 1234567890,
            bonding_curve_progress: Some(50.0),
            initial_liquidity_sol: Some(10.0),
            initial_virtual_quote_reserves: None,
            token_total_supply: Some(1_000_000),
            block_time: Some(1234567890),
        }
    }

    fn create_test_account_update(
        bonding_curve: Pubkey,
        write_version: Option<u64>,
        account_data_hash: &str,
    ) -> SeerEvent {
        SeerEvent::AccountUpdate(DetectedAccountUpdateEvent {
            provider_id: Some("raw-primary".to_string()),
            provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
            semantic: ghost_core::EventSemanticEnvelope::default(),
            event_time: ghost_core::EventTimeMetadata::default(),
            base_mint: Pubkey::new_unique(),
            bonding_curve,
            curve_finality: ghost_core::CurveFinality::Provisional,
            sol_reserves: 1_000,
            token_reserves: 2_000,
            real_sol_reserves: Some(300),
            real_token_reserves: Some(400),
            complete: 0,
            slot: 123,
            write_version,
            txn_signature: Some(solana_sdk::signature::Signature::new_unique()),
            account_data_hash: Some(account_data_hash.to_string()),
            account_data_len: Some(56),
            source_account_pubkey: Some(bonding_curve),
            source_account_owner_or_program: Some(Pubkey::new_unique()),
            replay_origin: AccountUpdateReplayOrigin::Live,
            replay_buffer_dwell_ms: None,
            detected_at: std::time::SystemTime::now(),
            sequence_number: u64::MAX,
        })
    }

    #[tokio::test]
    async fn test_channel_creation() {
        let config = IpcChannelConfig::default();
        let (sender, mut receiver, metrics) = create_ipc_channel(config);

        let candidate = create_test_candidate();
        let original_slot = candidate.slot;
        sender
            .send(candidate.clone(), EventPriority::Normal)
            .await
            .unwrap();

        let event = receiver.recv().await.unwrap();
        match event {
            SeerEvent::PoolDetected(pool_event) => {
                assert_eq!(pool_event.candidate.slot, original_slot);
                assert_eq!(pool_event.priority, EventPriority::Normal);
            }
            _ => panic!("Expected PoolDetected event"),
        }
        assert_eq!(metrics.events_sent.get(), 1);
        assert_eq!(metrics.events_received.get(), 1);
    }

    #[tokio::test]
    async fn send_account_update_carries_account_data_hash_metadata() {
        let config = IpcChannelConfig::default();
        let (sender, mut receiver, _metrics) = create_ipc_channel(config);
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let txn_signature = solana_sdk::signature::Signature::new_unique();

        sender
            .send_account_update(
                Some("raw-primary".to_string()),
                Some(RawProviderRoleV1::PrimaryAuthority),
                ghost_core::EventSemanticEnvelope::default(),
                ghost_core::EventTimeMetadata::default(),
                base_mint,
                bonding_curve,
                ghost_core::CurveFinality::Provisional,
                1_000,
                2_000,
                Some(300),
                Some(400),
                0,
                123,
                Some(7),
                Some(txn_signature),
                Some("raw-blake3".to_string()),
                Some(56),
                Some(bonding_curve),
                Some(owner),
                AccountUpdateReplayOrigin::Live,
                None,
            )
            .await
            .expect("account update should send");

        let Some(SeerEvent::AccountUpdate(event)) = receiver.recv().await else {
            panic!("expected account update event");
        };
        assert_eq!(event.account_data_hash.as_deref(), Some("raw-blake3"));
        assert_eq!(event.account_data_len, Some(56));
        assert_eq!(event.source_account_pubkey, Some(bonding_curve));
        assert_eq!(event.source_account_owner_or_program, Some(owner));
        assert_eq!(event.write_version, Some(7));
        assert_eq!(event.provider_id.as_deref(), Some("raw-primary"));
        assert_eq!(
            event.provider_role,
            Some(RawProviderRoleV1::PrimaryAuthority)
        );
        assert_eq!(event.txn_signature, Some(txn_signature));
        assert_eq!(event.real_sol_reserves, Some(300));
        assert_eq!(event.real_token_reserves, Some(400));
    }

    #[tokio::test]
    async fn canonical_account_update_survives_full_downstream_and_arrives_once() {
        let config = IpcChannelConfig {
            buffer_size: 1,
            account_update_queue_capacity: 4,
            ..IpcChannelConfig::default()
        };
        let (sender, mut receiver, _metrics) = create_ipc_channel(config);

        sender
            .send(create_test_candidate(), EventPriority::Normal)
            .await
            .expect("seed downstream");
        wait_for_downstream_len(&receiver, 1).await;

        sender
            .send(create_test_candidate(), EventPriority::Normal)
            .await
            .expect("dispatcher may wait behind full downstream");
        wait_for_dispatcher_len(&sender, 0).await;

        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let started = std::time::Instant::now();
        sender
            .send_account_update(
                Some("raw-primary".to_string()),
                Some(RawProviderRoleV1::PrimaryAuthority),
                ghost_core::EventSemanticEnvelope::default(),
                ghost_core::EventTimeMetadata::default(),
                base_mint,
                bonding_curve,
                ghost_core::CurveFinality::Provisional,
                1_000,
                2_000,
                Some(300),
                Some(400),
                0,
                123,
                Some(7),
                Some(solana_sdk::signature::Signature::new_unique()),
                Some("raw-blake3".to_string()),
                Some(56),
                Some(bonding_curve),
                Some(Pubkey::new_unique()),
                AccountUpdateReplayOrigin::Live,
                None,
            )
            .await
            .expect("canonical update must enter the dedicated state lane");
        assert!(
            started.elapsed() < std::time::Duration::from_millis(50),
            "canonical AccountUpdate enqueue must not wait for downstream capacity"
        );

        assert!(matches!(
            receiver.recv().await,
            Some(SeerEvent::PoolDetected(_))
        ));
        assert!(matches!(
            receiver.recv().await,
            Some(SeerEvent::PoolDetected(_))
        ));
        let Some(SeerEvent::AccountUpdate(update)) = receiver.recv().await else {
            panic!("canonical AccountUpdate must be delivered after capacity is released");
        };
        assert_eq!(update.base_mint, base_mint);
        assert_eq!(update.bonding_curve, bonding_curve);
        assert_eq!(update.write_version, Some(7));

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), receiver.recv())
                .await
                .is_err(),
            "the preserved canonical update must arrive exactly once"
        );
        sender
            .shutdown_and_join(Duration::from_secs(1))
            .expect("IPC dispatcher should drain and join");
    }

    #[test]
    fn account_update_fifo_retains_same_version_conflicts_and_none_separately() {
        let queue = IpcEgressQueue::new(4, 4);
        let bonding_curve = Pubkey::new_unique();

        queue
            .try_enqueue(create_test_account_update(bonding_curve, None, "hash-none"))
            .expect("unknown write-version observation must be admitted");
        queue
            .try_enqueue(create_test_account_update(
                bonding_curve,
                Some(0),
                "hash-zero-a",
            ))
            .expect("Some(0) observation must remain distinct from None");
        queue
            .try_enqueue(create_test_account_update(
                bonding_curve,
                Some(0),
                "hash-zero-b",
            ))
            .expect("same-version/different-hash observation must be retained");

        queue.begin_shutdown(Duration::from_secs(1));
        let mut observations = Vec::new();
        while let Some(SeerEvent::AccountUpdate(update)) = queue.next_event() {
            observations.push((
                update.sequence_number,
                update.write_version,
                update.account_data_hash,
            ));
        }

        assert_eq!(
            observations,
            vec![
                (0, None, Some("hash-none".to_string())),
                (1, Some(0), Some("hash-zero-a".to_string())),
                (2, Some(0), Some("hash-zero-b".to_string())),
            ]
        );
    }

    #[test]
    fn concurrent_multi_lane_enqueue_is_globally_sequence_ordered() {
        const PRODUCERS: usize = 64;

        let queue = Arc::new(IpcEgressQueue::new(PRODUCERS, PRODUCERS));
        let barrier = Arc::new(std::sync::Barrier::new(PRODUCERS + 1));
        let bonding_curve = Pubkey::new_unique();
        let mut handles = Vec::with_capacity(PRODUCERS);

        for producer in 0..PRODUCERS {
            let queue = Arc::clone(&queue);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                let event = if producer % 2 == 0 {
                    create_test_account_update(
                        bonding_curve,
                        Some(producer as u64),
                        &format!("hash-{producer}"),
                    )
                } else {
                    SeerEvent::PoolDetected(DetectedPoolEvent {
                        candidate: create_test_candidate(),
                        observation: None,
                        runtime_disposition: PoolDetectionRuntimeDispositionV1::CandidateAdmission,
                        continuity_observation_pool: None,
                        detected_at: std::time::SystemTime::now(),
                        sequence_number: u64::MAX,
                        priority: EventPriority::Normal,
                    })
                };
                barrier.wait();
                queue.try_enqueue(event).expect("concurrent enqueue");
            }));
        }

        barrier.wait();
        for handle in handles {
            handle.join().expect("producer thread");
        }
        queue.begin_shutdown(Duration::from_secs(1));

        let mut observed_sequences = Vec::with_capacity(PRODUCERS);
        while let Some(event) = queue.next_event() {
            observed_sequences.push(seer_event_sequence(&event));
        }
        assert_eq!(
            observed_sequences,
            (0..PRODUCERS as u64).collect::<Vec<_>>(),
            "dispatcher merge must preserve the enqueue linearization order"
        );
    }

    #[tokio::test]
    async fn shutdown_is_bounded_when_downstream_stops_consuming() {
        let config = IpcChannelConfig {
            buffer_size: 1,
            account_update_queue_capacity: 1,
            ..IpcChannelConfig::default()
        };
        let (sender, receiver, _metrics) = create_ipc_channel(config);

        sender
            .send(create_test_candidate(), EventPriority::Normal)
            .await
            .expect("fill downstream");
        wait_for_downstream_len(&receiver, 1).await;
        sender
            .send(create_test_candidate(), EventPriority::Normal)
            .await
            .expect("dispatcher owns one pending event");
        wait_for_dispatcher_len(&sender, 0).await;

        let timeout = Duration::from_millis(40);
        let started = Instant::now();
        let result = sender.shutdown_and_join(timeout);
        assert!(
            matches!(result, Err(IpcError::ShutdownTimeout { .. })),
            "stalled downstream must produce an explicit shutdown timeout: {result:?}"
        );
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "shutdown must not wait indefinitely for a stalled downstream"
        );
        drop(receiver);
    }

    #[test]
    fn detected_account_update_event_old_json_defaults_hash_metadata_to_none() {
        let event = DetectedAccountUpdateEvent {
            provider_id: Some("raw-primary".to_string()),
            provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
            semantic: ghost_core::EventSemanticEnvelope::default(),
            event_time: ghost_core::EventTimeMetadata::default(),
            base_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            curve_finality: ghost_core::CurveFinality::Provisional,
            sol_reserves: 1_000,
            token_reserves: 2_000,
            real_sol_reserves: None,
            real_token_reserves: None,
            complete: 0,
            slot: 123,
            write_version: Some(7),
            txn_signature: Some(solana_sdk::signature::Signature::new_unique()),
            account_data_hash: Some("raw-blake3".to_string()),
            account_data_len: Some(56),
            source_account_pubkey: Some(Pubkey::new_unique()),
            source_account_owner_or_program: Some(Pubkey::new_unique()),
            replay_origin: AccountUpdateReplayOrigin::Live,
            replay_buffer_dwell_ms: None,
            detected_at: std::time::SystemTime::now(),
            sequence_number: 1,
        };
        let mut value = serde_json::to_value(event).expect("serialize event");
        let object = value
            .as_object_mut()
            .expect("account update should serialize as object");
        object.remove("provider_id");
        object.remove("provider_role");
        object.remove("txn_signature");
        object.remove("account_data_hash");
        object.remove("account_data_len");
        object.remove("source_account_pubkey");
        object.remove("source_account_owner_or_program");

        let decoded: DetectedAccountUpdateEvent =
            serde_json::from_value(value).expect("deserialize old account update shape");
        assert_eq!(decoded.provider_id, None);
        assert_eq!(decoded.provider_role, None);
        assert_eq!(decoded.txn_signature, None);
        assert_eq!(decoded.account_data_hash, None);
        assert_eq!(decoded.account_data_len, None);
        assert_eq!(decoded.source_account_pubkey, None);
        assert_eq!(decoded.source_account_owner_or_program, None);
    }

    #[tokio::test]
    async fn candidate_admission_pool_detected_overrides_drop_new_with_a_coverage_gap() {
        let config = IpcChannelConfig {
            buffer_size: 2,
            backpressure_policy: BackpressurePolicy::DropNew,
            log_drops: false,
            ..Default::default()
        };
        let (sender, receiver, metrics) = create_ipc_channel(config);
        let mut local_gap_rx = receiver.local_coverage_gap_receiver();

        // Fill the existing downstream IPC channel, let the fixed dispatcher
        // hold one event, then fill the bounded egress queue deterministically.
        let candidate = create_test_candidate();
        for expected in 1..=2 {
            sender
                .send(candidate.clone(), EventPriority::Normal)
                .await
                .unwrap();
            wait_for_downstream_len(&receiver, expected).await;
        }
        sender
            .send(candidate.clone(), EventPriority::Normal)
            .await
            .unwrap();
        wait_for_dispatcher_len(&sender, 0).await;
        for expected in 1..=2 {
            sender
                .send(candidate.clone(), EventPriority::Normal)
                .await
                .unwrap();
            wait_for_dispatcher_len(&sender, expected).await;
        }

        // CandidateAdmission is structural primary traffic: even with a
        // DropNew config it must report a gap rather than silently disappear.
        let result = sender.send(candidate, EventPriority::Normal).await;
        assert!(matches!(result, Err(IpcError::LocalProcessingGap)));
        assert_eq!(metrics.events_dropped.get(), 0);
        tokio::time::timeout(Duration::from_secs(1), local_gap_rx.changed())
            .await
            .expect("structural pool saturation must reach coverage control plane")
            .expect("coverage control sender remains alive");
        let notices = local_gap_rx.borrow_and_update().clone();
        assert!(notices
            .notices
            .iter()
            .any(|notice| notice.reason
                == ghost_core::LocalCoverageGapReasonV1::IpcEgressQueueSaturated));
    }

    #[tokio::test]
    async fn test_drop_by_priority() {
        let config = IpcChannelConfig {
            buffer_size: 2,
            backpressure_policy: BackpressurePolicy::DropByPriority,
            log_drops: false,
            ..Default::default()
        };
        let (sender, receiver, metrics) = create_ipc_channel(config);

        // Fill the existing downstream IPC channel, let the fixed dispatcher
        // hold one event, then fill the bounded egress queue deterministically.
        let candidate = create_test_candidate();
        for expected in 1..=2 {
            sender
                .send(candidate.clone(), EventPriority::Normal)
                .await
                .unwrap();
            wait_for_downstream_len(&receiver, expected).await;
        }
        sender
            .send(candidate.clone(), EventPriority::Normal)
            .await
            .unwrap();
        wait_for_dispatcher_len(&sender, 0).await;
        for expected in 1..=2 {
            sender
                .send(candidate.clone(), EventPriority::Normal)
                .await
                .unwrap();
            wait_for_dispatcher_len(&sender, expected).await;
        }

        // Only non-admission continuity traffic retains DropByPriority.
        let result = sender
            .send_with_observation_and_disposition(
                candidate.clone(),
                None,
                PoolDetectionRuntimeDispositionV1::ContinuityOnly,
                None,
                EventPriority::Low,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(metrics.drops_by_priority_low.get(), 1);
    }

    #[test]
    fn test_metrics_calculation() {
        let metrics = IpcMetrics::new();

        // Simulate sending and dropping
        for _ in 0..100 {
            metrics.events_sent.inc();
        }
        for _ in 0..5 {
            metrics.record_drop(EventPriority::Low);
        }

        let drop_rate = metrics.calculate_drop_rate();
        assert_eq!(drop_rate, 5.0); // 5 drops out of 100 sent = 5%
    }

    #[test]
    fn test_queue_utilization() {
        let metrics = IpcMetrics::new();
        metrics.update_queue_length(800);

        let utilization = metrics.calculate_queue_utilization(1000);
        assert_eq!(utilization, 80.0);
    }

    // =============================================================================
    // Trade Event IPC Tests
    // =============================================================================

    fn create_test_trade_event(is_buy: bool) -> crate::types::TradeEvent {
        use solana_sdk::signature::Signature;

        crate::types::TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            provider_id: None,
            provider_role: None,
            slot: Some(12345),
            signature: Signature::new_unique(),
            event_ordinal: Some(0),
            tx_index: None,
            provenance: None,
            timestamp_ms: 1234567890000,
            arrival_ts_ms: 1234567890001,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: Pubkey::new_unique(),
            mint: Pubkey::new_unique(),
            signer: Pubkey::new_unique(),
            is_buy,
            is_dev_buy: false,
            amount: 1000000,
            max_sol_cost: if is_buy { 5000000 } else { 0 },
            min_sol_output: if is_buy { 0 } else { 3000000 },
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![1, 2, 3, 4, 5],
            mpcf_payload_missing_reason: crate::types::RawBytesMissingReason::Unknown,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            virtual_sol_reserves: None,
            virtual_token_reserves: None,
            real_sol_reserves: None,
            real_token_reserves: None,
            complete: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            creator_vault: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            buy_remaining_accounts: vec![],
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: crate::types::ToolchainFingerprintInput::default(),
            curve_data_known: false,
            curve_finality: ghost_core::CurveFinality::Speculative,
            is_pumpswap: false,
        }
    }

    fn create_test_funding_transfer_event() -> FundingTransferEvent {
        FundingTransferEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(12345),
            event_ordinal: Some(3),
            tx_index: None,
            outer_instruction_index: Some(1),
            inner_group_index: Some(1),
            cpi_stack_height: Some(2),
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1_234_567_890_001,
            signature: "funding-sig".to_string(),
            source_wallet: Pubkey::new_unique().to_string(),
            recipient_wallet: Pubkey::new_unique().to_string(),
            lamports: 50_000_000,
            full_chain_coverage: false,
            provenance: FundingTransferProvenance::filtered_grpc_global_stream_live(),
        }
    }

    fn create_test_execution_account_evidence() -> ExecutionAccountEvidence {
        ExecutionAccountEvidence {
            role: ghost_core::ExecutionAccountRole::BondingCurveV2,
            account_pubkey: Pubkey::new_unique(),
            base_mint: Some(Pubkey::new_unique()),
            pool_id: Some(Pubkey::new_unique()),
            canonical_bonding_curve: Some(Pubkey::new_unique()),
            source: ghost_core::ExecutionAccountEvidenceSource::RpcHydration,
            status: ghost_core::ExecutionAccountEvidenceStatus::RpcReady,
            slot: Some(12345),
            context_slot: Some(12346),
            write_version: Some(7),
            owner: Some(Pubkey::new_unique()),
            data_len: Some(256),
            tx_signature: Some("evidence-sig".to_string()),
            observed_instruction_index: Some(2),
            observed_account_position: Some(9),
            provenance_status: Some("route_compatible".to_string()),
            detected_at_ms: 1_234_567_890_000,
            received_at_ms: 1_234_567_890_010,
            evidence_ready: true,
            reason: None,
        }
    }

    #[tokio::test]
    async fn test_trade_event_ipc_buy() {
        let config = IpcChannelConfig::default();
        let (sender, mut receiver, metrics) = create_ipc_channel(config);

        // Create a Buy trade event
        let trade = create_test_trade_event(true);
        let original_slot = trade.slot;
        let original_amount = trade.amount;
        let original_is_buy = trade.is_buy;
        let original_max_sol_cost = trade.max_sol_cost;

        // Send trade via IPC
        sender
            .send_trade(trade.clone(), EventPriority::Normal)
            .await
            .unwrap();

        // Receive and verify
        let received_event = receiver.recv().await.unwrap();

        match received_event {
            SeerEvent::Trade(trade_event) => {
                assert_eq!(trade_event.trade.slot, original_slot);
                assert_eq!(trade_event.trade.amount, original_amount);
                assert_eq!(trade_event.trade.is_buy, original_is_buy);
                assert_eq!(trade_event.trade.max_sol_cost, original_max_sol_cost);
                assert_eq!(trade_event.trade.min_sol_output, 0);
                assert_eq!(trade_event.priority, EventPriority::Normal);
            }
            _ => panic!("Expected SeerEvent::Trade, got pool event"),
        }

        assert_eq!(metrics.events_sent.get(), 1);
        assert_eq!(metrics.events_received.get(), 1);
    }

    #[tokio::test]
    async fn test_trade_event_ipc_sell() {
        let config = IpcChannelConfig::default();
        let (sender, mut receiver, metrics) = create_ipc_channel(config);

        // Create a Sell trade event
        let trade = create_test_trade_event(false);
        let original_slot = trade.slot;
        let original_amount = trade.amount;
        let original_is_buy = trade.is_buy;
        let original_min_sol_output = trade.min_sol_output;

        // Send trade via IPC
        sender
            .send_trade(trade.clone(), EventPriority::Normal)
            .await
            .unwrap();

        // Receive and verify
        let received_event = receiver.recv().await.unwrap();

        match received_event {
            SeerEvent::Trade(trade_event) => {
                assert_eq!(trade_event.trade.slot, original_slot);
                assert_eq!(trade_event.trade.amount, original_amount);
                assert_eq!(trade_event.trade.is_buy, original_is_buy);
                assert_eq!(trade_event.trade.max_sol_cost, 0);
                assert_eq!(trade_event.trade.min_sol_output, original_min_sol_output);
                assert_eq!(trade_event.priority, EventPriority::Normal);
            }
            _ => panic!("Expected SeerEvent::Trade, got pool event"),
        }

        assert_eq!(metrics.events_sent.get(), 1);
        assert_eq!(metrics.events_received.get(), 1);
    }

    #[tokio::test]
    async fn test_funding_transfer_event_ipc_roundtrip() {
        let config = IpcChannelConfig::default();
        let (sender, mut receiver, metrics) = create_ipc_channel(config);

        let transfer = create_test_funding_transfer_event();
        let expected_signature = transfer.signature.clone();
        let expected_source = transfer.source_wallet.clone();
        let expected_recipient = transfer.recipient_wallet.clone();
        let expected_lamports = transfer.lamports;
        let expected_full_chain_coverage = transfer.full_chain_coverage;
        let expected_provenance = transfer.provenance;
        let expected_arrival_ts_ms = transfer.arrival_ts_ms;
        let expected_event_ordinal = transfer.event_ordinal;
        let expected_outer_instruction_index = transfer.outer_instruction_index;
        let expected_inner_group_index = transfer.inner_group_index;
        let expected_cpi_stack_height = transfer.cpi_stack_height;

        sender
            .send_funding_transfer(transfer, EventPriority::High)
            .await
            .unwrap();

        let received_event = receiver.recv().await.unwrap();
        match received_event {
            SeerEvent::FundingTransfer(funding_event) => {
                assert_eq!(funding_event.transfer.signature, expected_signature);
                assert_eq!(funding_event.transfer.source_wallet, expected_source);
                assert_eq!(funding_event.transfer.recipient_wallet, expected_recipient);
                assert_eq!(funding_event.transfer.lamports, expected_lamports);
                assert_eq!(
                    funding_event.transfer.full_chain_coverage,
                    expected_full_chain_coverage
                );
                assert_eq!(funding_event.transfer.provenance, expected_provenance);
                assert_eq!(funding_event.transfer.arrival_ts_ms, expected_arrival_ts_ms);
                assert_eq!(funding_event.transfer.event_ordinal, expected_event_ordinal);
                assert_eq!(
                    funding_event.transfer.outer_instruction_index,
                    expected_outer_instruction_index
                );
                assert_eq!(
                    funding_event.transfer.inner_group_index,
                    expected_inner_group_index
                );
                assert_eq!(
                    funding_event.transfer.cpi_stack_height,
                    expected_cpi_stack_height
                );
                assert_eq!(funding_event.priority, EventPriority::High);
            }
            other => panic!("Expected SeerEvent::FundingTransfer, got {:?}", other),
        }

        assert_eq!(metrics.events_sent.get(), 1);
        assert_eq!(metrics.events_received.get(), 1);
    }

    #[tokio::test]
    async fn test_execution_account_evidence_event_ipc_roundtrip() {
        let config = IpcChannelConfig::default();
        let (sender, mut receiver, metrics) = create_ipc_channel(config);

        let evidence = create_test_execution_account_evidence();
        let expected = evidence.clone();

        sender
            .send_execution_account_evidence(evidence, EventPriority::High)
            .await
            .unwrap();

        let received_event = receiver.recv().await.unwrap();
        match received_event {
            SeerEvent::ExecutionAccountEvidence(event) => {
                assert_eq!(event.evidence, expected);
                assert_eq!(
                    event.evidence.role,
                    ghost_core::ExecutionAccountRole::BondingCurveV2
                );
                assert_eq!(
                    event.evidence.source,
                    ghost_core::ExecutionAccountEvidenceSource::RpcHydration
                );
                assert_eq!(
                    event.evidence.status,
                    ghost_core::ExecutionAccountEvidenceStatus::RpcReady
                );
                assert!(event.evidence.evidence_ready);
                assert_eq!(event.sequence_number, 0);
                assert_eq!(event.priority, EventPriority::High);
            }
            other => panic!(
                "Expected SeerEvent::ExecutionAccountEvidence, got {:?}",
                other
            ),
        }

        assert_eq!(metrics.events_sent.get(), 1);
        assert_eq!(metrics.events_received.get(), 1);
    }

    #[tokio::test]
    async fn test_mixed_pool_and_trade_events() {
        let config = IpcChannelConfig::default();
        let (sender, mut receiver, metrics) = create_ipc_channel(config);

        // Send pool event
        let candidate = create_test_candidate();
        sender
            .send(candidate.clone(), EventPriority::Normal)
            .await
            .unwrap();

        // Send trade event
        let trade = create_test_trade_event(true);
        sender
            .send_trade(trade.clone(), EventPriority::High)
            .await
            .unwrap();

        // Send another pool event
        let candidate2 = create_test_candidate();
        sender
            .send(candidate2.clone(), EventPriority::Normal)
            .await
            .unwrap();

        // Receive and verify order
        let event1 = receiver.recv().await.unwrap();
        match event1 {
            SeerEvent::PoolDetected(pool_event) => {
                assert_eq!(pool_event.candidate.slot, candidate.slot);
                assert_eq!(pool_event.priority, EventPriority::Normal);
            }
            _ => panic!("Expected first event to be PoolDetected"),
        }

        let event2 = receiver.recv().await.unwrap();
        match event2 {
            SeerEvent::Trade(trade_event) => {
                assert_eq!(trade_event.trade.slot, trade.slot);
                assert_eq!(trade_event.priority, EventPriority::High);
            }
            _ => panic!("Expected second event to be Trade"),
        }

        let event3 = receiver.recv().await.unwrap();
        match event3 {
            SeerEvent::PoolDetected(pool_event) => {
                assert_eq!(pool_event.candidate.slot, candidate2.slot);
                assert_eq!(pool_event.priority, EventPriority::Normal);
            }
            _ => panic!("Expected third event to be PoolDetected"),
        }

        assert_eq!(metrics.events_sent.get(), 3);
        assert_eq!(metrics.events_received.get(), 3);
    }

    #[test]
    fn test_seer_event_serialization_deserialization_pool() {
        use std::time::SystemTime;

        let candidate = create_test_candidate();
        let pool_event = DetectedPoolEvent {
            candidate: candidate.clone(),
            observation: None,
            runtime_disposition: PoolDetectionRuntimeDispositionV1::CandidateAdmission,
            continuity_observation_pool: None,
            detected_at: SystemTime::now(),
            sequence_number: 42,
            priority: EventPriority::High,
        };

        let seer_event = SeerEvent::PoolDetected(pool_event);

        // Serialize
        let serialized = serde_json::to_string(&seer_event)
            .expect("Failed to serialize SeerEvent::PoolDetected");

        // Deserialize
        let deserialized: SeerEvent = serde_json::from_str(&serialized)
            .expect("Failed to deserialize SeerEvent::PoolDetected");

        // Verify
        match deserialized {
            SeerEvent::PoolDetected(pool_event) => {
                assert_eq!(pool_event.candidate.slot, candidate.slot);
                assert_eq!(pool_event.sequence_number, 42);
                assert_eq!(pool_event.priority, EventPriority::High);
            }
            _ => panic!("Deserialized wrong variant"),
        }
    }

    #[test]
    fn legacy_pool_and_trade_json_default_raw_observation_to_none() {
        use std::time::SystemTime;

        let mut pool_json = serde_json::to_value(SeerEvent::PoolDetected(DetectedPoolEvent {
            candidate: create_test_candidate(),
            observation: None,
            runtime_disposition: PoolDetectionRuntimeDispositionV1::CandidateAdmission,
            continuity_observation_pool: None,
            detected_at: SystemTime::now(),
            sequence_number: 1,
            priority: EventPriority::Normal,
        }))
        .expect("serialize pool event");
        let legacy_pool = pool_json
            .get_mut("PoolDetected")
            .and_then(serde_json::Value::as_object_mut)
            .expect("externally tagged pool event");
        legacy_pool.remove("observation");
        legacy_pool.remove("runtime_disposition");
        legacy_pool.remove("continuity_observation_pool");
        match serde_json::from_value::<SeerEvent>(pool_json).expect("legacy pool JSON") {
            SeerEvent::PoolDetected(event) => {
                assert_eq!(event.observation, None);
                assert_eq!(
                    event.runtime_disposition,
                    PoolDetectionRuntimeDispositionV1::CandidateAdmission
                );
                assert_eq!(event.continuity_observation_pool, None);
            }
            other => panic!("expected pool event, got {other:?}"),
        }

        let mut trade_json = serde_json::to_value(SeerEvent::Trade(DetectedTradeEvent {
            trade: create_test_trade_event(true),
            observation: None,
            detected_at: SystemTime::now(),
            sequence_number: 2,
            priority: EventPriority::Normal,
        }))
        .expect("serialize trade event");
        trade_json
            .get_mut("Trade")
            .and_then(serde_json::Value::as_object_mut)
            .expect("externally tagged trade event")
            .remove("observation");
        match serde_json::from_value::<SeerEvent>(trade_json).expect("legacy trade JSON") {
            SeerEvent::Trade(event) => assert_eq!(event.observation, None),
            other => panic!("expected trade event, got {other:?}"),
        }
    }

    #[test]
    fn test_seer_event_serialization_deserialization_trade() {
        use std::time::SystemTime;

        let trade = create_test_trade_event(true);
        let original_slot = trade.slot;
        let original_amount = trade.amount;
        let original_pool_amm_id = trade.pool_amm_id;
        let original_mint = trade.mint;
        let original_signer = trade.signer;

        let trade_event = DetectedTradeEvent {
            trade: trade.clone(),
            observation: None,
            detected_at: SystemTime::now(),
            sequence_number: 99,
            priority: EventPriority::Normal,
        };

        let seer_event = SeerEvent::Trade(trade_event);

        // Serialize
        let serialized =
            serde_json::to_string(&seer_event).expect("Failed to serialize SeerEvent::Trade");

        // Deserialize
        let deserialized: SeerEvent =
            serde_json::from_str(&serialized).expect("Failed to deserialize SeerEvent::Trade");

        // Verify all fields match
        match deserialized {
            SeerEvent::Trade(trade_event) => {
                assert_eq!(trade_event.trade.slot, original_slot);
                assert_eq!(trade_event.trade.amount, original_amount);
                assert_eq!(trade_event.trade.pool_amm_id, original_pool_amm_id);
                assert_eq!(trade_event.trade.mint, original_mint);
                assert_eq!(trade_event.trade.signer, original_signer);
                assert_eq!(trade_event.trade.is_buy, true);
                assert_eq!(trade_event.trade.max_sol_cost, 5000000);
                assert_eq!(trade_event.trade.min_sol_output, 0);
                assert_eq!(trade_event.trade.mpcf_payload, vec![1, 2, 3, 4, 5]);
                assert_eq!(trade_event.sequence_number, 99);
                assert_eq!(trade_event.priority, EventPriority::Normal);
            }
            _ => panic!("Deserialized wrong variant"),
        }
    }

    #[test]
    fn test_seer_event_bincode_serialization_trade() {
        use std::time::SystemTime;

        let trade = create_test_trade_event(false);

        let trade_event = DetectedTradeEvent {
            trade: trade.clone(),
            observation: None,
            detected_at: SystemTime::now(),
            sequence_number: 123,
            priority: EventPriority::Low,
        };

        let seer_event = SeerEvent::Trade(trade_event);

        // Serialize with bincode (more efficient binary format)
        let serialized =
            bincode::serialize(&seer_event).expect("Failed to bincode serialize SeerEvent::Trade");
        assert!(!serialized.is_empty());
    }

    #[test]
    fn test_seer_event_serialization_deserialization_funding_transfer() {
        use std::time::SystemTime;

        let transfer = create_test_funding_transfer_event();
        let expected_signature = transfer.signature.clone();
        let expected_source = transfer.source_wallet.clone();
        let expected_recipient = transfer.recipient_wallet.clone();
        let expected_lamports = transfer.lamports;
        let expected_full_chain_coverage = transfer.full_chain_coverage;
        let expected_provenance = transfer.provenance;
        let expected_arrival_ts_ms = transfer.arrival_ts_ms;
        let expected_event_ordinal = transfer.event_ordinal;
        let expected_outer_instruction_index = transfer.outer_instruction_index;
        let expected_inner_group_index = transfer.inner_group_index;
        let expected_cpi_stack_height = transfer.cpi_stack_height;

        let funding_event = DetectedFundingTransferEvent {
            transfer,
            lane_health: FundingLaneRuntimeHealth::default(),
            detected_at: SystemTime::now(),
            sequence_number: 77,
            priority: EventPriority::High,
        };

        let seer_event = SeerEvent::FundingTransfer(funding_event);
        let serialized = serde_json::to_string(&seer_event)
            .expect("Failed to serialize SeerEvent::FundingTransfer");
        let deserialized: SeerEvent = serde_json::from_str(&serialized)
            .expect("Failed to deserialize SeerEvent::FundingTransfer");

        match deserialized {
            SeerEvent::FundingTransfer(funding_event) => {
                assert_eq!(funding_event.transfer.signature, expected_signature);
                assert_eq!(funding_event.transfer.source_wallet, expected_source);
                assert_eq!(funding_event.transfer.recipient_wallet, expected_recipient);
                assert_eq!(funding_event.transfer.lamports, expected_lamports);
                assert_eq!(
                    funding_event.transfer.full_chain_coverage,
                    expected_full_chain_coverage
                );
                assert_eq!(funding_event.transfer.provenance, expected_provenance);
                assert_eq!(funding_event.transfer.arrival_ts_ms, expected_arrival_ts_ms);
                assert_eq!(funding_event.transfer.event_ordinal, expected_event_ordinal);
                assert_eq!(
                    funding_event.transfer.outer_instruction_index,
                    expected_outer_instruction_index
                );
                assert_eq!(
                    funding_event.transfer.inner_group_index,
                    expected_inner_group_index
                );
                assert_eq!(
                    funding_event.transfer.cpi_stack_height,
                    expected_cpi_stack_height
                );
                assert_eq!(funding_event.sequence_number, 77);
                assert_eq!(funding_event.priority, EventPriority::High);
            }
            other => panic!("Deserialized wrong variant: {:?}", other),
        }
    }

    #[test]
    fn test_seer_event_serialization_deserialization_execution_account_evidence() {
        use std::time::SystemTime;

        let evidence = create_test_execution_account_evidence();
        let expected = evidence.clone();
        let evidence_event = DetectedExecutionAccountEvidenceEvent {
            evidence,
            detected_at: SystemTime::now(),
            sequence_number: 88,
            priority: EventPriority::High,
        };

        let seer_event = SeerEvent::ExecutionAccountEvidence(evidence_event);
        let serialized = serde_json::to_string(&seer_event)
            .expect("Failed to serialize SeerEvent::ExecutionAccountEvidence");
        let deserialized: SeerEvent = serde_json::from_str(&serialized)
            .expect("Failed to deserialize SeerEvent::ExecutionAccountEvidence");

        match deserialized {
            SeerEvent::ExecutionAccountEvidence(event) => {
                assert_eq!(event.evidence, expected);
                assert_eq!(event.evidence.role.label(), "bonding_curve_v2");
                assert_eq!(event.evidence.source.as_str(), "rpc_hydration");
                assert_eq!(event.evidence.status.as_str(), "rpc_ready");
                assert_eq!(event.sequence_number, 88);
                assert_eq!(event.priority, EventPriority::High);
            }
            other => panic!("Deserialized wrong variant: {:?}", other),
        }
    }

    #[test]
    fn test_filtered_funding_transfer_serialization_omits_default_provenance() {
        let transfer = create_test_funding_transfer_event();
        let serialized = serde_json::to_value(&transfer).expect("serialize funding transfer");
        let object = serialized
            .as_object()
            .expect("funding transfer must serialize as JSON object");
        assert!(
            !object.contains_key("provenance"),
            "default filtered provenance should stay omitted for legacy JSON shape"
        );
    }

    #[test]
    fn test_legacy_funding_transfer_fixture_deserializes_with_filtered_defaults() {
        let transfer = create_test_funding_transfer_event();
        let funding_event = DetectedFundingTransferEvent {
            transfer,
            lane_health: FundingLaneRuntimeHealth::default(),
            detected_at: std::time::SystemTime::now(),
            sequence_number: 77,
            priority: EventPriority::High,
        };
        let mut serialized = serde_json::to_value(SeerEvent::FundingTransfer(funding_event))
            .expect("serialize fixture");

        let outer = serialized
            .as_object_mut()
            .expect("seer event must serialize as object");
        let inner = outer
            .get_mut("FundingTransfer")
            .and_then(serde_json::Value::as_object_mut)
            .expect("funding transfer variant payload must serialize as object");
        let transfer_object = inner
            .get_mut("transfer")
            .and_then(serde_json::Value::as_object_mut)
            .expect("transfer payload must serialize as object");
        transfer_object.remove("provenance");

        let deserialized: SeerEvent =
            serde_json::from_value(serialized).expect("legacy fixture should deserialize");
        match deserialized {
            SeerEvent::FundingTransfer(event) => {
                assert!(!event.transfer.full_chain_coverage);
                assert_eq!(
                    event.transfer.provenance,
                    FundingTransferProvenance::filtered_grpc_global_stream_live()
                );
            }
            other => panic!("expected funding transfer, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_trade_event_with_all_priorities() {
        let config = IpcChannelConfig::default();
        let (sender, mut receiver, _metrics) = create_ipc_channel(config);

        let priorities = vec![
            EventPriority::Low,
            EventPriority::Normal,
            EventPriority::High,
        ];

        for priority in priorities {
            let trade = create_test_trade_event(true);
            sender.send_trade(trade.clone(), priority).await.unwrap();

            let received = receiver.recv().await.unwrap();
            match received {
                SeerEvent::Trade(trade_event) => {
                    assert_eq!(trade_event.priority, priority);
                }
                _ => panic!("Expected Trade event"),
            }
        }
    }

    #[tokio::test]
    async fn test_trade_event_backpressure_drop_new() {
        let config = IpcChannelConfig {
            buffer_size: 2,
            backpressure_policy: BackpressurePolicy::DropNew,
            log_drops: false,
            ..Default::default()
        };
        let (sender, receiver, metrics) = create_ipc_channel(config);
        let mut local_gap_rx = receiver.local_coverage_gap_receiver();

        // Critical trades override DropNew. They never wait for downstream
        // capacity: saturation opens a typed local-processing gap and fails
        // closed instead of reporting delivery.
        for expected in 1..=2 {
            sender
                .send_trade(create_test_trade_event(true), EventPriority::Normal)
                .await
                .unwrap();
            wait_for_downstream_len(&receiver, expected).await;
        }
        sender
            .send_trade(create_test_trade_event(true), EventPriority::Normal)
            .await
            .unwrap();
        wait_for_dispatcher_len(&sender, 0).await;
        for expected in 1..=2 {
            sender
                .send_trade(create_test_trade_event(true), EventPriority::Normal)
                .await
                .unwrap();
            wait_for_dispatcher_len(&sender, expected).await;
        }

        let send_res = sender
            .send_trade(create_test_trade_event(true), EventPriority::Normal)
            .await;
        assert!(matches!(send_res, Err(IpcError::LocalProcessingGap)));
        assert_eq!(metrics.events_dropped.get(), 0);
        assert!(sender.has_unrecovered_local_gap());
        tokio::time::timeout(Duration::from_secs(1), local_gap_rx.changed())
            .await
            .expect("queue saturation must notify the independent control plane")
            .expect("coverage-gap control sender remains alive");
        let notices = local_gap_rx.borrow_and_update().clone();
        let notice = notices.notices.first().expect("coverage-gap notice");
        assert!(!notices.overflowed);
        assert_eq!(notice.provider_id, "unknown");
        assert_eq!(
            notice.reason,
            ghost_core::LocalCoverageGapReasonV1::IpcEgressQueueSaturated
        );
    }

    #[tokio::test]
    async fn local_coverage_gap_control_retains_primary_after_a_witness_notice() {
        let (sender, receiver, _metrics) = create_ipc_channel(IpcChannelConfig::default());
        let mut gap_rx = receiver.local_coverage_gap_receiver();

        sender.report_local_coverage_gap(
            "secondary",
            ghost_core::LocalCoverageGapReasonV1::IpcEgressQueueSaturated,
        );
        sender.report_local_coverage_gap(
            "primary",
            ghost_core::LocalCoverageGapReasonV1::IngressQueueSaturated,
        );

        tokio::time::timeout(Duration::from_secs(1), gap_rx.changed())
            .await
            .expect("coverage control state changes")
            .expect("coverage-gap sender remains alive");
        let state = gap_rx.borrow_and_update().clone();
        assert!(!state.overflowed);
        assert_eq!(
            state
                .notices
                .iter()
                .map(|notice| notice.provider_id.as_str())
                .collect::<Vec<_>>(),
            vec!["secondary", "primary"],
            "a later witness/control notice must never overwrite a primary gap"
        );
    }

    #[tokio::test]
    async fn local_coverage_gap_control_marks_overflow_without_unbounded_retention() {
        let (sender, receiver, _metrics) = create_ipc_channel(IpcChannelConfig::default());
        let mut gap_rx = receiver.local_coverage_gap_receiver();

        for index in 0..=MAX_RETAINED_LOCAL_COVERAGE_GAP_NOTICES {
            sender.report_local_coverage_gap(
                format!("provider-{index}"),
                ghost_core::LocalCoverageGapReasonV1::IpcEgressQueueSaturated,
            );
        }

        tokio::time::timeout(Duration::from_secs(1), gap_rx.changed())
            .await
            .expect("coverage control state changes")
            .expect("coverage-gap sender remains alive");
        let state = gap_rx.borrow_and_update().clone();
        assert_eq!(
            state.notices.len(),
            MAX_RETAINED_LOCAL_COVERAGE_GAP_NOTICES,
            "control-plane retention remains bounded"
        );
        assert!(
            state.overflowed,
            "an unretained provider gap must be explicit so launcher can fail closed"
        );
    }
}
