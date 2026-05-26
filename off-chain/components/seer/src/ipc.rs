//! IPC layer for Seer→Trigger communication
//!
//! This module provides a typed event channel with backpressure handling
//! and comprehensive metrics for monitoring the event pipeline.

use crate::types::{CandidatePool, TradeEvent};
use ghost_core::{
    CurveFinality, EventSemanticEnvelope, EventTimeMetadata, ExecutionAccountEvidence,
};
use prometheus::{
    register_histogram, register_int_counter, register_int_gauge, Histogram, IntCounter, IntGauge,
};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{error, warn};

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

/// Typed pool detection event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedPoolEvent {
    /// The detected pool candidate
    pub candidate: CandidatePool,

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

/// Typed funding-transfer event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedFundingTransferEvent {
    /// The funding transfer observation.
    pub transfer: FundingTransferEvent,

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

    /// Curve completion flag (1 = graduated, 0 = active).
    pub complete: u8,

    /// Slot at which this AccountUpdate was observed.
    pub slot: u64,

    /// Optional Solana account write-version from Yellowstone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_version: Option<u64>,

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
}

impl Default for IpcChannelConfig {
    fn default() -> Self {
        Self {
            buffer_size: 10000, // Large buffer for high-throughput bursts
            backpressure_policy: BackpressurePolicy::Block,
            log_drops: true,
            log_overflows: true,
            warning_threshold_percent: 80.0,
        }
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
}

/// Sender wrapper with backpressure handling and metrics
#[derive(Clone)]
pub struct IpcSender {
    /// Underlying channel sender
    sender: mpsc::Sender<SeerEvent>,

    /// Configuration
    config: IpcChannelConfig,

    /// Metrics
    metrics: Arc<IpcMetrics>,

    /// Sequence counter for events
    sequence_counter: Arc<std::sync::atomic::AtomicU64>,
}

impl IpcSender {
    /// Create a new IPC sender
    pub fn new(
        sender: mpsc::Sender<SeerEvent>,
        config: IpcChannelConfig,
        metrics: Arc<IpcMetrics>,
    ) -> Self {
        Self {
            sender,
            config,
            metrics,
            sequence_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Send a pool detection event through the channel with backpressure handling
    pub async fn send(
        &self,
        candidate: CandidatePool,
        priority: EventPriority,
    ) -> Result<(), IpcError> {
        let sequence = self
            .sequence_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let event = SeerEvent::PoolDetected(DetectedPoolEvent {
            candidate,
            detected_at: std::time::SystemTime::now(),
            sequence_number: sequence,
            priority,
        });

        self.send_event(event, priority).await
    }

    /// Send a trade event through the channel with backpressure handling
    pub async fn send_trade(
        &self,
        trade: TradeEvent,
        priority: EventPriority,
    ) -> Result<(), IpcError> {
        let sequence = self
            .sequence_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let event = SeerEvent::Trade(DetectedTradeEvent {
            trade,
            detected_at: std::time::SystemTime::now(),
            sequence_number: sequence,
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
        let sequence = self
            .sequence_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let event = SeerEvent::FundingTransfer(DetectedFundingTransferEvent {
            transfer,
            detected_at: std::time::SystemTime::now(),
            sequence_number: sequence,
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
    /// This is a critical path in the post-migration architecture, so the sender
    /// blocks under pressure instead of silently dropping fresh canonical state.
    ///
    /// `sol_reserves` / `token_reserves` must be the canonical virtual reserves
    /// from the bonding-curve account, not the real balance subset.
    pub async fn send_account_update(
        &self,
        semantic: EventSemanticEnvelope,
        event_time: EventTimeMetadata,
        base_mint: Pubkey,
        bonding_curve: Pubkey,
        curve_finality: CurveFinality,
        sol_reserves: u64,
        token_reserves: u64,
        complete: u8,
        slot: u64,
        write_version: Option<u64>,
        replay_origin: AccountUpdateReplayOrigin,
        replay_buffer_dwell_ms: Option<u64>,
    ) -> Result<(), IpcError> {
        let sequence = self
            .sequence_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let event = SeerEvent::AccountUpdate(DetectedAccountUpdateEvent {
            semantic,
            event_time,
            base_mint,
            bonding_curve,
            curve_finality,
            sol_reserves,
            token_reserves,
            complete,
            slot,
            write_version,
            replay_origin,
            replay_buffer_dwell_ms,
            detected_at: std::time::SystemTime::now(),
            sequence_number: sequence,
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
        let sequence = self
            .sequence_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let event = SeerEvent::ExecutionAccountEvidence(DetectedExecutionAccountEvidenceEvent {
            evidence,
            detected_at: std::time::SystemTime::now(),
            sequence_number: sequence,
            priority,
        });

        self.send_event_with_policy(event, priority, BackpressurePolicy::Block)
            .await
    }

    #[must_use]
    pub fn current_queue_length(&self) -> usize {
        self.metrics.queue_length.get().max(0) as usize
    }

    /// Internal method to send any SeerEvent
    async fn send_event(&self, event: SeerEvent, priority: EventPriority) -> Result<(), IpcError> {
        self.send_event_with_policy(event, priority, self.config.backpressure_policy)
            .await
    }

    async fn send_event_with_policy(
        &self,
        event: SeerEvent,
        priority: EventPriority,
        policy: BackpressurePolicy,
    ) -> Result<(), IpcError> {
        // Extract sequence number for logging
        let sequence = match &event {
            SeerEvent::PoolDetected(e) => e.sequence_number,
            SeerEvent::Trade(e) => e.sequence_number,
            SeerEvent::FundingTransfer(e) => e.sequence_number,
            SeerEvent::AccountUpdate(e) => e.sequence_number,
            SeerEvent::ExecutionAccountEvidence(e) => e.sequence_number,
        };

        // Calculate actual queue length from remaining capacity
        // Note: sender.capacity() returns REMAINING permits, not current queue length
        // So: queue_length = buffer_size - remaining_capacity
        let remaining_capacity = self.sender.capacity();
        let current_queue_length = self.config.buffer_size.saturating_sub(remaining_capacity);
        self.metrics.update_queue_length(current_queue_length);

        // Check if we're approaching capacity
        let utilization = self
            .metrics
            .calculate_queue_utilization(self.config.buffer_size);
        if utilization >= self.config.warning_threshold_percent && self.config.log_overflows {
            warn!(
                "IPC queue utilization high: {:.1}% ({}/{})",
                utilization, current_queue_length, self.config.buffer_size
            );
        }

        // Apply backpressure policy
        let send_result = match policy {
            BackpressurePolicy::Block => {
                // Block until space is available
                self.sender
                    .send(event)
                    .await
                    .map_err(|e| IpcError::SendError(e.to_string()))
            }
            BackpressurePolicy::DropNew => {
                // Try to send, drop if full
                match self.sender.try_send(event.clone()) {
                    Ok(_) => Ok(()),
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        self.metrics.record_drop(priority);
                        if self.config.log_drops {
                            error!(
                                "Dropped event (seq={}, priority={:?}): queue full (DropNew policy)",
                                sequence, priority
                            );
                        }
                        Err(IpcError::EventDropped {
                            policy: BackpressurePolicy::DropNew,
                            priority,
                        })
                    }
                    Err(e) => Err(IpcError::SendError(e.to_string())),
                }
            }
            BackpressurePolicy::DropOldest => {
                // Try to send, if full, this would require custom implementation
                // For simplicity, we'll treat this as Block for now since tokio mpsc doesn't support dropping oldest
                warn!("DropOldest policy not fully implemented, using Block instead");
                self.sender
                    .send(event)
                    .await
                    .map_err(|e| IpcError::SendError(e.to_string()))
            }
            BackpressurePolicy::DropByPriority => {
                // Try to send, drop low priority first
                match self.sender.try_send(event.clone()) {
                    Ok(_) => Ok(()),
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        // Only drop if priority is Low, otherwise block
                        if priority == EventPriority::Low {
                            self.metrics.record_drop(priority);
                            if self.config.log_drops {
                                error!("Dropped Low priority event (seq={}): queue full", sequence);
                            }
                            Err(IpcError::EventDropped {
                                policy: BackpressurePolicy::DropByPriority,
                                priority,
                            })
                        } else {
                            // Block for Normal/High priority
                            self.sender
                                .send(event)
                                .await
                                .map_err(|e| IpcError::SendError(e.to_string()))
                        }
                    }
                    Err(e) => Err(IpcError::SendError(e.to_string())),
                }
            }
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
        self.metrics
            .calculate_queue_utilization(self.config.buffer_size)
    }

    /// Get drop rate
    pub fn drop_rate(&self) -> f64 {
        self.metrics.calculate_drop_rate()
    }
}

/// Receiver wrapper with metrics tracking
pub struct IpcReceiver {
    /// Underlying channel receiver
    receiver: mpsc::Receiver<SeerEvent>,

    /// Metrics
    metrics: Arc<IpcMetrics>,
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

impl IpcReceiver {
    /// Create a new IPC receiver
    pub fn new(receiver: mpsc::Receiver<SeerEvent>, metrics: Arc<IpcMetrics>) -> Self {
        Self { receiver, metrics }
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
    let (tx, rx) = mpsc::channel(config.buffer_size);
    let metrics = IpcMetrics::new();

    let sender = IpcSender::new(tx, config.clone(), Arc::clone(&metrics));
    let receiver = IpcReceiver::new(rx, Arc::clone(&metrics));

    (sender, receiver, metrics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CandidatePool;
    use solana_sdk::pubkey::Pubkey;

    fn create_test_candidate() -> CandidatePool {
        CandidatePool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(100),
            event_ts_ms: Some(1_234_567_890_000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: "test_sig".to_string(),
            amm_program_id: Pubkey::new_unique(),
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            timestamp: 1234567890,
            bonding_curve_progress: Some(50.0),
            initial_liquidity_sol: Some(10.0),
            token_total_supply: Some(1_000_000),
            block_time: Some(1234567890),
        }
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
    async fn test_drop_new_policy() {
        let config = IpcChannelConfig {
            buffer_size: 2,
            backpressure_policy: BackpressurePolicy::DropNew,
            log_drops: false,
            ..Default::default()
        };
        let (sender, _receiver, metrics) = create_ipc_channel(config);

        // Fill the buffer
        let candidate = create_test_candidate();
        sender
            .send(candidate.clone(), EventPriority::Normal)
            .await
            .unwrap();
        sender
            .send(candidate.clone(), EventPriority::Normal)
            .await
            .unwrap();

        // This should be dropped
        let result = sender.send(candidate, EventPriority::Normal).await;
        assert!(result.is_err());
        assert_eq!(metrics.events_dropped.get(), 1);
    }

    #[tokio::test]
    async fn test_drop_by_priority() {
        let config = IpcChannelConfig {
            buffer_size: 2,
            backpressure_policy: BackpressurePolicy::DropByPriority,
            log_drops: false,
            ..Default::default()
        };
        let (sender, _receiver, metrics) = create_ipc_channel(config);

        // Fill the buffer
        let candidate = create_test_candidate();
        sender
            .send(candidate.clone(), EventPriority::Normal)
            .await
            .unwrap();
        sender
            .send(candidate.clone(), EventPriority::Normal)
            .await
            .unwrap();

        // Low priority should be dropped
        let result = sender.send(candidate.clone(), EventPriority::Low).await;
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
            slot: Some(12345),
            signature: Signature::new_unique(),
            event_ordinal: Some(0),
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
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
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
        let (sender, mut receiver, metrics) = create_ipc_channel(config);

        // Fill the buffer with trades
        let trade1 = create_test_trade_event(true);
        sender
            .send_trade(trade1, EventPriority::Normal)
            .await
            .unwrap();

        let trade2 = create_test_trade_event(false);
        sender
            .send_trade(trade2, EventPriority::Normal)
            .await
            .unwrap();

        // Trade events are lossless even under DropNew policy.
        let trade3 = create_test_trade_event(true);
        let (send_res, _ev) =
            tokio::join!(sender.send_trade(trade3, EventPriority::Normal), async {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                receiver.recv().await.expect("receiver should make room")
            });
        send_res.unwrap();
        assert_eq!(metrics.events_dropped.get(), 0);
    }
}
