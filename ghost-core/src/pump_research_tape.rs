//! Frozen V1.1 contracts for the Pump Research Evidence Tape.
//!
//! This module is deliberately data-only.  It must not become a live runtime
//! authority, a Yellowstone adapter, a parser, or a writer.  PR-A owns capture
//! and PR-B owns materialisation; CS0 owns the durable contracts that both
//! stages must honour.
//!
//! # V1 immutability rule
//!
//! `PumpResearchRawRecordV1`, `PumpRawSegmentHeaderV1`,
//! `PumpRawSegmentClosedV1`, and every nested storage type in this module are
//! permanently frozen after CS0.  A storage change requires a V2 record,
//! decoder, and artifact directory; it must never be made additively to V1.

use crate::{
    CanonicalPumpOrderKeyV1, EventTimeMetadata, PumpTradeSideV1, RawPumpMutationLocatorV1,
};
use bincode::Options;
use serde::{
    de::{self, DeserializeOwned, SeqAccess, Visitor},
    ser::SerializeTuple,
    Deserialize, Deserializer, Serialize, Serializer,
};
use solana_sdk::pubkey::Pubkey;
use std::{fmt, marker::PhantomData};
use thiserror::Error;

/// Immutable binary storage format selected by a V1 raw manifest, segment
/// header, and segment footer.
pub const PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1: u16 = 1;

/// The V1 raw record payload limit.  The limit applies before the 4-byte
/// length prefix and 32-byte payload digest are added.
pub const PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1: usize = 16 * 1024 * 1024;

/// Magic prefix of a V1 segment before its framed header.
pub const PUMP_RESEARCH_RAW_SEGMENT_MAGIC_V1: [u8; 8] = *b"PRTAPE01";

/// Capture semantics intentionally exclude original gRPC wire-frame identity
/// and protobuf unknown fields.
pub const PUMP_RESEARCH_SOURCE_CAPTURE_SEMANTICS_V1: &str = "decoded_protobuf_schema_lossless_v1";

/// Frozen dependency and source-schema identities from the local Cargo.lock.
pub const PUMP_RESEARCH_SOURCE_PROTO_SCHEMA_VERSION_V1: &str = "yellowstone-geyser-proto-v1";
pub const PUMP_RESEARCH_SOURCE_PROTO_CRATE_V1: &str = "yellowstone-grpc-proto";
pub const PUMP_RESEARCH_SOURCE_PROTO_CRATE_VERSION_V1: &str = "1.14.2";
pub const PUMP_RESEARCH_SOURCE_CLIENT_CRATE_V1: &str = "yellowstone-grpc-client";
pub const PUMP_RESEARCH_SOURCE_CLIENT_VERSION_V1: &str = "1.15.4";
pub const PUMP_RESEARCH_PROST_VERSION_V1: &str = "0.12.6";
pub const PUMP_RESEARCH_BINCODE_VERSION_V1: &str = "1.3.3";
pub const PUMP_RESEARCH_PROGRAM_DATA_HASH_ALGORITHM_V1: &str = "blake3-256";
pub const PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1: &str =
    "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1: &str =
    "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf";
pub const PUMP_RESEARCH_SOURCE_DESCRIPTOR_RELATIVE_PATH_V1: &str =
    "ghost-core/tests/fixtures/pump_research_tape_v1/yellowstone_v1_descriptor.pb";
pub const PUMP_RESEARCH_SOURCE_DESCRIPTOR_SHA256_HEX_V1: &str =
    "9b92e4810f4af0d100f268b31d52d0cedf55dfee8c6b512f43b7698205450acb";
pub const PUMP_RESEARCH_SOURCE_PROTO_DESCRIPTOR_HASH_V1: &str =
    "sha256:9b92e4810f4af0d100f268b31d52d0cedf55dfee8c6b512f43b7698205450acb";

/// Fixed-width storage bytes.  This wrapper prevents a future `serde`
/// implementation change in a Solana domain type from changing V1 bytes.
///
/// The custom tuple encoding is deliberate: bincode writes exactly `N` bytes,
/// without a `Vec`-style length prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PumpResearchFixedBytesV1<const N: usize>(pub [u8; N]);

impl<const N: usize> PumpResearchFixedBytesV1<N> {
    #[must_use]
    pub const fn new(bytes: [u8; N]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn into_inner(self) -> [u8; N] {
        self.0
    }

    #[must_use]
    pub const fn as_array(&self) -> &[u8; N] {
        &self.0
    }
}

impl<const N: usize> Default for PumpResearchFixedBytesV1<N> {
    fn default() -> Self {
        Self([0; N])
    }
}

impl<const N: usize> From<[u8; N]> for PumpResearchFixedBytesV1<N> {
    fn from(value: [u8; N]) -> Self {
        Self(value)
    }
}

impl<const N: usize> Serialize for PumpResearchFixedBytesV1<N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tuple = serializer.serialize_tuple(N)?;
        for byte in &self.0 {
            tuple.serialize_element(byte)?;
        }
        tuple.end()
    }
}

struct FixedBytesVisitor<const N: usize>(PhantomData<[u8; N]>);

impl<'de, const N: usize> Visitor<'de> for FixedBytesVisitor<N> {
    type Value = PumpResearchFixedBytesV1<N>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "exactly {N} fixed storage bytes")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut bytes = [0u8; N];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = sequence
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(index, &self))?;
        }
        Ok(PumpResearchFixedBytesV1(bytes))
    }
}

impl<'de, const N: usize> Deserialize<'de> for PumpResearchFixedBytesV1<N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple(N, FixedBytesVisitor(PhantomData))
    }
}

/// Stable storage representation of a Solana pubkey.
pub type PumpResearchStoragePubkeyV1 = PumpResearchFixedBytesV1<32>;
/// Stable storage representation of a Solana signature.
pub type PumpResearchStorageSignatureV1 = PumpResearchFixedBytesV1<64>;
/// Stable storage representation of a BLAKE3 or SHA-256 digest.
pub type PumpResearchStorageHashV1 = PumpResearchFixedBytesV1<32>;

#[must_use]
pub fn pump_research_storage_pubkey_v1(pubkey: Pubkey) -> PumpResearchStoragePubkeyV1 {
    PumpResearchStoragePubkeyV1::new(pubkey.to_bytes())
}

#[must_use]
pub fn pump_research_pubkey_from_storage_v1(value: PumpResearchStoragePubkeyV1) -> Pubkey {
    Pubkey::new_from_array(value.into_inner())
}

/// Storage-owned representation of the three time axes.  It intentionally
/// mirrors, but does not serialize through, the live `EventTimeMetadata` type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpResearchEventTimeV1 {
    pub chain_event_ts_ms: Option<u64>,
    pub ingress_wall_ts_ms: Option<u64>,
    pub ingress_monotonic_ts_ms: Option<u64>,
}

impl From<EventTimeMetadata> for PumpResearchEventTimeV1 {
    fn from(value: EventTimeMetadata) -> Self {
        Self {
            chain_event_ts_ms: value.chain_event_ts_ms,
            ingress_wall_ts_ms: value.ingress_wall_ts_ms,
            ingress_monotonic_ts_ms: value.ingress_monotonic_ts_ms,
        }
    }
}

impl From<PumpResearchEventTimeV1> for EventTimeMetadata {
    fn from(value: PumpResearchEventTimeV1) -> Self {
        Self::new(
            value.chain_event_ts_ms,
            value.ingress_wall_ts_ms,
            value.ingress_monotonic_ts_ms,
        )
    }
}

/// V1 raw capture has exactly one primary evidence authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpResearchProviderRoleV1 {
    PrimaryAuthority,
}

/// Provenance shared by every source-derived raw record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpRawSourceEnvelopeV1 {
    pub provider_id: String,
    pub provider_role: PumpResearchProviderRoleV1,
    pub stream_epoch: u64,
    pub capture_sequence: u64,
    /// BLAKE3-256 of the deterministic `prost` re-encoding of the decoded
    /// source payload, never of an original gRPC/HTTP2 wire frame.
    pub payload_hash_blake3: PumpResearchStorageHashV1,
}

/// Account class retained by the raw tape.  Global is deliberately an account
/// update role, not a separate V1 record variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpResearchAccountRoleV1 {
    BondingCurve,
    TransitionDependencyGlobal,
}

/// Source-lossless transaction evidence.  `source_payload` contains the
/// deterministic protobuf encoding of `SubscribeUpdateTransaction`; therefore
/// it retains accounts, outer/inner instructions, logs, loaded addresses,
/// balance metadata and all frozen-schema fields without a second RPC fetch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpPrimaryTransactionEvidenceV1 {
    pub source: PumpRawSourceEnvelopeV1,
    pub slot: u64,
    /// `None` remains an explicit source limitation; `Some(0)` is valid.
    pub tx_index: Option<u32>,
    pub signature: PumpResearchStorageSignatureV1,
    pub event_time: PumpResearchEventTimeV1,
    pub block_time: Option<i64>,
    pub source_payload: Vec<u8>,
}

/// Source-lossless Pump-owned account evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpPrimaryAccountUpdateEvidenceV1 {
    pub source: PumpRawSourceEnvelopeV1,
    pub account_role: PumpResearchAccountRoleV1,
    pub is_startup: bool,
    pub account_pubkey: PumpResearchStoragePubkeyV1,
    pub owner_program: PumpResearchStoragePubkeyV1,
    pub raw_account_data: Vec<u8>,
    pub raw_account_data_hash_blake3: PumpResearchStorageHashV1,
    pub slot: u64,
    pub write_version: u64,
    pub txn_signature: Option<PumpResearchStorageSignatureV1>,
    pub event_time: PumpResearchEventTimeV1,
    pub source_payload: Vec<u8>,
}

/// Raw slot evidence.  `source_status` is the frozen protobuf numeric enum;
/// this raw record must not pre-classify a slot as canonical or dead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpPrimarySlotEvidenceV1 {
    pub source: PumpRawSourceEnvelopeV1,
    pub slot: u64,
    pub parent: Option<u64>,
    pub source_status: i32,
    pub event_time: PumpResearchEventTimeV1,
    pub source_payload: Vec<u8>,
}

/// Raw block-meta evidence used for chain-time and slot-continuity proof.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpPrimaryBlockMetaEvidenceV1 {
    pub source: PumpRawSourceEnvelopeV1,
    pub slot: u64,
    pub parent_slot: u64,
    pub block_time: Option<i64>,
    pub event_time: PumpResearchEventTimeV1,
    pub source_payload: Vec<u8>,
}

/// Fixed-width persistence form of a local gap boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpRawCoverageBoundaryV1 {
    pub slot: Option<u64>,
    pub signature: Option<PumpResearchStorageSignatureV1>,
}

/// Persistence adapter for `LocalCoverageGapV1`.  It does not create another
/// runtime gap authority.  The additional record-limit reason only exists for
/// the V1 raw artifact admission boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpRawCoverageGapReasonV1 {
    IngressQueueSaturated,
    WalQueueSaturated,
    EvidenceQueueSaturated,
    IpcEgressQueueSaturated,
    RecordExceedsFrozenLimit,
}

/// Frozen persistence form of one continuous local coverage gap episode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpRawCoverageGapV1 {
    pub gap_id_blake3: PumpResearchStorageHashV1,
    pub provider_id: String,
    pub stream_epoch: u64,
    pub episode_sequence: u64,
    pub reason: PumpRawCoverageGapReasonV1,
    pub before: PumpRawCoverageBoundaryV1,
    pub after: PumpRawCoverageBoundaryV1,
    pub missing_event_count: u64,
    pub first_dropped: PumpRawCoverageBoundaryV1,
    pub last_dropped: PumpRawCoverageBoundaryV1,
    pub queue_high_water: u64,
    pub started_at_wall_ms: u64,
    pub ended_at_wall_ms: u64,
    pub recovered: bool,
}

/// Durable end marker for an atomically published V1 segment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpRawSegmentClosedV1 {
    pub storage_format_version: u16,
    pub segment_index: u64,
    pub accepted_record_count: u64,
    pub data_bytes: u64,
    pub segment_blake3: PumpResearchStorageHashV1,
    pub closed_wall_ts_ms: u64,
    pub clean_shutdown: bool,
}

/// The only V1 raw record enum.  Its declaration order is physical storage
/// layout and must never change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpResearchRawRecordV1 {
    PrimaryTransaction(PumpPrimaryTransactionEvidenceV1),
    PrimaryAccountUpdate(PumpPrimaryAccountUpdateEvidenceV1),
    PrimarySlotUpdate(PumpPrimarySlotEvidenceV1),
    PrimaryBlockMeta(PumpPrimaryBlockMetaEvidenceV1),
    CoverageGap(PumpRawCoverageGapV1),
    SegmentClosed(PumpRawSegmentClosedV1),
}

/// Segment metadata encoded immediately after `PUMP_RESEARCH_RAW_SEGMENT_MAGIC_V1`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpRawSegmentHeaderV1 {
    pub storage_format_version: u16,
    pub run_id: String,
    pub segment_index: u64,
    pub stream_epoch: u64,
    pub opened_wall_ts_ms: u64,
    pub opened_monotonic_ts_ms: u64,
    pub previous_segment_blake3: Option<PumpResearchStorageHashV1>,
}

/// Program and ProgramData evidence from one finalized, read-only RPC receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpProgramDataReceiptV1 {
    pub pump_program_id: PumpResearchStoragePubkeyV1,
    pub pump_program_account_owner: PumpResearchStoragePubkeyV1,
    pub pump_programdata_pubkey: PumpResearchStoragePubkeyV1,
    pub program_data_owner: PumpResearchStoragePubkeyV1,
    pub program_data_hash_algorithm: String,
    pub program_data_hash_blake3: PumpResearchStorageHashV1,
    pub program_deployment_slot: Option<u64>,
    pub observed_context_slot: u64,
    pub commitment: String,
}

/// Immutable metadata created before the source stream is opened.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpResearchRunStartManifestV1 {
    pub storage_format_version: u16,
    pub schema_version: u16,
    pub run_id: String,
    pub repository_commit: String,
    pub binary_hash_blake3: PumpResearchStorageHashV1,
    pub config_hash_blake3: PumpResearchStorageHashV1,
    pub raw_event_schema_version: u16,
    pub decoder_version: String,
    pub primary_provider_id: String,
    pub primary_provider_role: PumpResearchProviderRoleV1,
    pub commitment: String,
    pub subscription_request_fingerprint_blake3: PumpResearchStorageHashV1,
    pub stream_epoch: u64,
    pub capture_started_wall_ms: u64,
    pub capture_started_monotonic_ms: u64,
    pub time_contract_version: u16,
    pub required_for_run: bool,
    pub source_proto_schema_version: String,
    /// A `sha256:<hex>` identity of the committed `FileDescriptorSet`.
    pub source_proto_descriptor_hash: String,
    pub source_proto_crate: String,
    pub source_proto_crate_version: String,
    pub source_client_crate: String,
    pub source_client_version: String,
    pub source_capture_semantics: String,
    pub pump_program_id: PumpResearchStoragePubkeyV1,
    pub pump_program_account_owner: PumpResearchStoragePubkeyV1,
    pub pump_programdata_pubkey: PumpResearchStoragePubkeyV1,
    pub program_data_owner: PumpResearchStoragePubkeyV1,
    pub program_data_hash_algorithm: String,
    pub program_data_hash_at_start: PumpResearchStorageHashV1,
    pub program_deployment_slot_at_start: Option<u64>,
    pub program_observed_context_slot_at_start: u64,
    pub program_receipt_commitment: String,
}

/// A completed segment referenced by the immutable completion receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpResearchSegmentReceiptV1 {
    pub segment_index: u64,
    pub filename: String,
    pub file_sha256: PumpResearchStorageHashV1,
    pub file_blake3: PumpResearchStorageHashV1,
    pub first_capture_sequence: Option<u64>,
    pub last_capture_sequence: Option<u64>,
    pub accepted_record_count: u64,
}

/// Completion state is evidence, not an opportunity to modify start metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpResearchRunCompletionStatusV1 {
    Complete,
    Incomplete,
    ProgramVersionBoundary,
}

/// Immutable completion receipt written only after writer drain and the final
/// ProgramData receipt attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpResearchRunCompletionReceiptV1 {
    pub storage_format_version: u16,
    pub run_id: String,
    pub capture_ended_wall_ms: u64,
    pub pump_program_id_at_completion: Option<PumpResearchStoragePubkeyV1>,
    pub pump_program_account_owner_at_completion: Option<PumpResearchStoragePubkeyV1>,
    pub pump_programdata_pubkey_at_completion: Option<PumpResearchStoragePubkeyV1>,
    pub program_data_owner_at_completion: Option<PumpResearchStoragePubkeyV1>,
    pub program_data_hash_at_completion: Option<PumpResearchStorageHashV1>,
    pub program_deployment_slot_at_completion: Option<u64>,
    pub program_observed_context_slot_at_completion: Option<u64>,
    pub program_receipt_commitment_at_completion: Option<String>,
    pub segment_list: Vec<PumpResearchSegmentReceiptV1>,
    pub gap_count: u64,
    /// Research-source lifecycle evidence.  These fields live in the JSON
    /// completion receipt, not in the permanently frozen V1 binary segment
    /// layout; defaults preserve readability of pre-correction receipts.
    #[serde(default)]
    pub source_stream_established: bool,
    #[serde(default)]
    pub first_source_update_received: bool,
    #[serde(default)]
    pub source_workers_cleanly_stopped: bool,
    #[serde(default)]
    pub received_source_update_count: u64,
    #[serde(default)]
    pub admitted_source_update_count: u64,
    #[serde(default)]
    pub persisted_source_record_count: u64,
    #[serde(default)]
    pub dropped_source_update_count: u64,
    #[serde(default)]
    pub persisted_ingress_gap_episode_count: u64,
    #[serde(default)]
    pub persisted_ingress_gap_missing_event_count: u64,
    #[serde(default)]
    pub source_lifecycle_error: Option<String>,
    #[serde(default)]
    pub capture_failure: Option<String>,
    pub clean_shutdown: bool,
    pub status: PumpResearchRunCompletionStatusV1,
}

/// Conservative output of the offline slot graph.  It is deliberately absent
/// from the raw record enum: raw slot evidence remains unmodified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpSlotCanonicalityV1 {
    RootedCanonical,
    Dead,
    Unresolved,
}

/// Distinct independent-audit invocation classes.  They are overlapping flags
/// at audit time; a single invocation may carry more than one class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PumpResearchSourceInvocationClassV1 {
    DirectTopLevel,
    InnerCpi,
    RouterToPumpCpi,
    V0LoadedAddress,
}

/// Canonical transaction identity used only to compare a read-only audit with
/// the raw tape.  Audit data cannot become source evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PumpResearchCanonicalTransactionIdentityV1 {
    pub slot: u64,
    pub tx_index: u32,
    pub signature: PumpResearchStorageSignatureV1,
}

/// The frozen non-writing contract of the independent source-completeness
/// audit.  PR-B may implement it but may not widen its authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpResearchQualificationAuditContractV1 {
    pub schema_version: u16,
    pub primary_provider_must_differ: bool,
    pub raw_tape_read_only: bool,
    pub audit_must_compare_failed_transactions: bool,
    pub audited_invocation_classes: Vec<PumpResearchSourceInvocationClassV1>,
}

/// Exactness reasons which do not themselves prove a conflicting value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpNonEvaluableReasonV1 {
    MissingPreAnchor,
    MissingFinalAnchor,
    MissingFinalTxnSignature,
    IncompleteMutationInventory,
    UnknownMutation,
    UnsupportedVariant,
    AmbiguousOrder,
    AmbiguousAmount,
    CoverageGap,
    ProcessBoundary,
    TruncatedSegment,
    FailedTransaction,
    NonCanonicalFork,
    UnresolvedCanonicality,
    SourceCoverageUnproven,
    SourceFilterCpiCoverageUnproven,
    ProgramVersionBoundary,
    TransitionDependencyUncaptured,
}

/// Exactness reasons where preserved evidence disagrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpConflictReasonV1 {
    AccountProviderConflict,
    AccountIdentityConflict,
    DirectEventStateMismatch,
    FinalStateMismatch,
    ConservationMismatch,
    MintCurveIdentityConflict,
}

/// State/order certification only.  Optional participant or protocol-flag
/// evidence must not downgrade a bit-exact reserve trajectory by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpTrajectoryCertificationV1 {
    Exact,
    NonEvaluable(PumpNonEvaluableReasonV1),
    Conflict(PumpConflictReasonV1),
}

/// Exact evidence requirement accepted by the generic exporter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpResearchRequiredEvidenceV1 {
    ParticipantBalance,
}

/// A window status records exclusion rather than silently dropping a launch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpResearchWindowStatusV1 {
    Complete,
    TruncatedAtRunStart,
    TruncatedAtRunEnd,
    CoverageGap,
    ProcessBoundary,
    NonExactMutation,
    MissingBirth,
    TerminalBeforeWindowEnd,
    NonCanonicalFork,
    UnresolvedCanonicality,
    ProgramVersionBoundary,
    MissingRequiredEvidence(PumpResearchRequiredEvidenceV1),
}

/// Qualification blockers retained in the exact manifest/report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpResearchQualificationBlockerV1 {
    SourceEvidenceInsufficient,
    /// The frozen tape does not contain a closed, non-empty interval bounded
    /// by preserved BlockMeta evidence inside every observed stream epoch.
    /// Qualification must never infer completeness across a mid-slot start,
    /// reconnect or unclosed shutdown tail.
    CaptureStreamBoundaryUnproven,
    /// Raw evidence may be replayed for development/forensics, but its
    /// run-local preflight binding does not prove the current sealed build and
    /// credential-isolation contract. An independent source audit cannot
    /// override this provenance boundary.
    CaptureProvenanceUnqualified,
    MutationInventoryIncomplete,
    TransitionSemanticsUnresolved,
    SourceFilterCpiCoverageUnproven,
    SourceCoverageUnproven,
    ProgramVersionBoundary,
    TransitionDependencyUncaptured,
    UnresolvedCanonicality,
    CreateV2Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpResearchTapeQualificationStatusV1 {
    Unqualified,
    /// Historical status retained for already-published external-audit
    /// artifacts. New GO-D research promotion does not depend on GO-E.
    Ready,
    /// The exact tape is derived exclusively from a hash-pinned, fully
    /// revalidated GO-D frozen raw tape. External GO-E/RPC evidence is not an
    /// input or a promotion gate.
    VerifiedFrozenTape,
    Blocked(PumpResearchQualificationBlockerV1),
}

/// Evidence status shared by optional participant and birth fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpEvidenceStatusV1 {
    Known,
    Unknown,
    Conflict,
}

/// Typed optional evidence.  `None` never means a numeric or boolean default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceValueV1<T> {
    pub value: Option<T>,
    pub status: PumpEvidenceStatusV1,
    pub source: Option<PumpEvidenceSourceV1>,
}

/// Sources permitted for generic optional evidence fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpEvidenceSourceV1 {
    CreatePayload,
    CreateV2Payload,
    AccountLayout,
    ProgramEvent,
    WitnessOnly,
    TransactionMeta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlagEvidenceStatusV1 {
    KnownTrue,
    KnownFalse,
    Unknown,
    Conflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlagEvidenceSourceV1 {
    CreatePayload,
    CreateV2Payload,
    AccountLayout,
    ProgramEvent,
    WitnessOnly,
}

/// Tri-state flag with source provenance; unknown is never coerced to false.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlagEvidenceV1 {
    pub value: Option<bool>,
    pub status: FlagEvidenceStatusV1,
    pub source: Option<FlagEvidenceSourceV1>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpCreateKindV1 {
    Create,
    CreateV2,
}

/// Curve state uses only raw on-chain units: lamports for quote and base units
/// for token reserves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpCurveStateV1 {
    pub virtual_quote_reserves: u64,
    pub virtual_token_reserves: u64,
    pub real_quote_reserves: u64,
    pub real_token_reserves: u64,
    pub complete: bool,
}

/// Stable location of an exact-tape source record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpRawSourceRefV1 {
    pub run_id: String,
    pub segment_index: u64,
    pub capture_sequence: u64,
    pub record_payload_hash_blake3: PumpResearchStorageHashV1,
}

/// Exact account anchor after replay through the existing account arbiter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpAccountAnchorV1 {
    pub source_ref: PumpRawSourceRefV1,
    pub account_pubkey: PumpResearchStoragePubkeyV1,
    pub slot: u64,
    pub write_version: u64,
    pub txn_signature: Option<PumpResearchStorageSignatureV1>,
    pub raw_account_data_hash_blake3: PumpResearchStorageHashV1,
    pub state: PumpCurveStateV1,
}

/// Mutation semantics recognized by the offline certifier.  `Unknown` is a
/// first-class structural inventory result, never an ignored instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpMutationKindV1 {
    Create,
    Trade,
    Complete,
    Withdraw,
    Migrate,
    UnknownMutation,
}

/// Frozen supported Pump instruction set for V1 materialisation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpInstructionVariantV1 {
    LegacyBuy,
    BuyV2,
    BuyExactQuoteInV2,
    LegacySell,
    SellV2,
    Create,
    CreateV2,
    Complete,
    Withdraw,
    Migrate,
    Unknown,
}

/// The only mutable reserve-state dependency permitted by V1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpTransitionDependencyV1 {
    None,
    PumpGlobal,
}

/// How a Create initial state was supplied to the certifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpCreateInitialStateEvidenceV1 {
    CompleteDirectCreateEvent,
    RequiresPumpGlobalFallback,
}

/// Frozen dependency closure.  Buy/sell reserve movements must never request
/// a current fee schedule; only incomplete Create/CreateV2 state may require
/// a historical Pump Global predecessor.
#[must_use]
pub const fn pump_transition_dependency_v1(
    variant: PumpInstructionVariantV1,
    create_initial_state: Option<PumpCreateInitialStateEvidenceV1>,
) -> PumpTransitionDependencyV1 {
    match (variant, create_initial_state) {
        (
            PumpInstructionVariantV1::Create | PumpInstructionVariantV1::CreateV2,
            Some(PumpCreateInitialStateEvidenceV1::RequiresPumpGlobalFallback),
        ) => PumpTransitionDependencyV1::PumpGlobal,
        _ => PumpTransitionDependencyV1::None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParticipantBalanceScopeV1 {
    CanonicalTradeTokenAccount,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParticipantBalanceProvenanceV1 {
    TransactionMetaAndCanonicalAtaProof {
        message_account_index: u32,
        instruction_account_position: u16,
        token_program: PumpResearchStoragePubkeyV1,
    },
    Unknown,
}

/// One transition in a transaction-local curve trajectory.  Settlement/fees
/// stay distinct from curve reserve movement and network transaction cost.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpCertifiedMutationV1 {
    pub locator: RawPumpMutationLocatorV1,
    pub order: CanonicalPumpOrderKeyV1,
    pub kind: PumpMutationKindV1,
    pub instruction_variant: PumpInstructionVariantV1,
    pub success: bool,
    pub error: Option<String>,
    pub participant: EvidenceValueV1<PumpResearchStoragePubkeyV1>,
    pub side: Option<PumpTradeSideV1>,
    pub token_amount_units: EvidenceValueV1<u64>,
    pub curve_quote_lamports: EvidenceValueV1<u64>,
    pub instruction_limit_lamports: Option<u64>,
    pub wallet_quote_delta_lamports: Option<u64>,
    pub protocol_fee_lamports: Option<u64>,
    pub creator_fee_lamports: Option<u64>,
    pub state_before: Option<PumpCurveStateV1>,
    pub state_after: Option<PumpCurveStateV1>,
    pub participant_token_account: EvidenceValueV1<PumpResearchStoragePubkeyV1>,
    pub participant_token_balance_before_units: EvidenceValueV1<u64>,
    pub participant_token_balance_after_units: EvidenceValueV1<u64>,
    pub participant_balance_scope: ParticipantBalanceScopeV1,
    pub participant_balance_provenance: ParticipantBalanceProvenanceV1,
}

/// One curve's ordered mutations inside a single transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpTransactionTrajectoryV1 {
    pub source_ref: PumpRawSourceRefV1,
    pub signature: PumpResearchStorageSignatureV1,
    pub slot: u64,
    pub tx_index: u32,
    pub event_time: PumpResearchEventTimeV1,
    pub mint: PumpResearchStoragePubkeyV1,
    pub bonding_curve: PumpResearchStoragePubkeyV1,
    pub pre_anchor: Option<PumpAccountAnchorV1>,
    pub mutations: Vec<PumpCertifiedMutationV1>,
    pub final_anchor: Option<PumpAccountAnchorV1>,
    pub certification: PumpTrajectoryCertificationV1,
}

/// Durable birth evidence without legacy creator conflation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpBirthEvidenceV1 {
    pub candidate_id: String,
    pub create_kind: PumpCreateKindV1,
    pub locator: RawPumpMutationLocatorV1,
    pub order: CanonicalPumpOrderKeyV1,
    pub event_time: PumpResearchEventTimeV1,
    pub mint: PumpResearchStoragePubkeyV1,
    pub bonding_curve: PumpResearchStoragePubkeyV1,
    pub quote_mint: PumpResearchStoragePubkeyV1,
    pub protocol_creator: EvidenceValueV1<PumpResearchStoragePubkeyV1>,
    pub create_user: EvidenceValueV1<PumpResearchStoragePubkeyV1>,
    pub initial_state: EvidenceValueV1<PumpCurveStateV1>,
    pub token_total_supply: EvidenceValueV1<u64>,
    pub mayhem: FlagEvidenceV1,
    pub cashback: FlagEvidenceV1,
}

/// Exact-tape metadata. Historical outputs may still carry `Ready` from the
/// retired GO-E path; current strategy research requires
/// `VerifiedFrozenTape` plus the explicit GO-D authority fields below.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactResearchTapeManifestV1 {
    pub schema_version: u16,
    pub source_run_id: String,
    pub source_storage_format_version: u16,
    pub qualification_status: PumpResearchTapeQualificationStatusV1,
    /// Additive JSON authority for exact outputs produced from one pinned raw
    /// segment snapshot. Empty only for historical/unqualified V1 manifests.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_raw_segment_set_blake3: String,
    /// Literal source-authority decision for final reports. Empty for
    /// historical exact manifests.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub go_d_source_authority: String,
    /// True only when GO-E was not used as a status or promotion gate.
    #[serde(default)]
    pub external_go_e_audit_not_used_as_gate: bool,
    /// SHA-256 of the create-new operator authority that pins this GO-D.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub go_d_source_authority_sha256: String,
    pub source_descriptor_sha256: String,
    pub program_start_receipt: PumpProgramDataReceiptV1,
    pub program_completion_receipt: Option<PumpProgramDataReceiptV1>,
}

/// Errors from the frozen bincode/framing implementation.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PumpResearchRawCodecErrorV1 {
    #[error("V1 raw {kind} payload is {actual_bytes} bytes, above frozen {max_bytes}-byte limit")]
    RecordTooLarge {
        kind: &'static str,
        actual_bytes: usize,
        max_bytes: usize,
    },
    #[error("V1 raw frame is {actual_bytes} bytes; expected exactly {expected_bytes}")]
    InvalidFrameLength {
        actual_bytes: usize,
        expected_bytes: usize,
    },
    #[error("V1 raw frame is shorter than its mandatory 4-byte length and 32-byte digest")]
    FrameTooShort,
    #[error("V1 raw payload digest mismatch")]
    PayloadHashMismatch,
    #[error("V1 raw segment magic mismatch")]
    SegmentMagicMismatch,
    #[error("V1 bincode failure: {message}")]
    Bincode { message: String },
}

/// Frozen raw frame codec:
/// `u32 little-endian payload_length | bincode-1.3.3 fixed-int little-endian
/// payload | BLAKE3-256(payload)`.
pub struct PumpResearchRawCodecV1;

impl PumpResearchRawCodecV1 {
    #[must_use]
    pub const fn storage_format_version() -> u16 {
        PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1
    }

    pub fn encode_record(
        record: &PumpResearchRawRecordV1,
    ) -> Result<Vec<u8>, PumpResearchRawCodecErrorV1> {
        encode_framed_v1(record, "record")
    }

    pub fn decode_record(
        frame: &[u8],
    ) -> Result<PumpResearchRawRecordV1, PumpResearchRawCodecErrorV1> {
        decode_framed_v1(frame, "record")
    }

    /// A segment starts with magic followed by one framed `PumpRawSegmentHeaderV1`.
    pub fn encode_segment_header(
        header: &PumpRawSegmentHeaderV1,
    ) -> Result<Vec<u8>, PumpResearchRawCodecErrorV1> {
        let frame = encode_framed_v1(header, "segment header")?;
        let mut encoded =
            Vec::with_capacity(PUMP_RESEARCH_RAW_SEGMENT_MAGIC_V1.len() + frame.len());
        encoded.extend_from_slice(&PUMP_RESEARCH_RAW_SEGMENT_MAGIC_V1);
        encoded.extend_from_slice(&frame);
        Ok(encoded)
    }

    pub fn decode_segment_header(
        encoded: &[u8],
    ) -> Result<PumpRawSegmentHeaderV1, PumpResearchRawCodecErrorV1> {
        if !encoded.starts_with(&PUMP_RESEARCH_RAW_SEGMENT_MAGIC_V1) {
            return Err(PumpResearchRawCodecErrorV1::SegmentMagicMismatch);
        }
        decode_framed_v1(
            &encoded[PUMP_RESEARCH_RAW_SEGMENT_MAGIC_V1.len()..],
            "segment header",
        )
    }
}

fn frozen_bincode_options() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
}

fn encode_framed_v1<T>(
    value: &T,
    kind: &'static str,
) -> Result<Vec<u8>, PumpResearchRawCodecErrorV1>
where
    T: Serialize,
{
    let payload = frozen_bincode_options().serialize(value).map_err(|error| {
        PumpResearchRawCodecErrorV1::Bincode {
            message: error.to_string(),
        }
    })?;
    if payload.len() > PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1 {
        return Err(PumpResearchRawCodecErrorV1::RecordTooLarge {
            kind,
            actual_bytes: payload.len(),
            max_bytes: PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1,
        });
    }

    let payload_len =
        u32::try_from(payload.len()).map_err(|_| PumpResearchRawCodecErrorV1::RecordTooLarge {
            kind,
            actual_bytes: payload.len(),
            max_bytes: PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1,
        })?;
    let mut frame = Vec::with_capacity(4 + payload.len() + 32);
    frame.extend_from_slice(&payload_len.to_le_bytes());
    frame.extend_from_slice(&payload);
    frame.extend_from_slice(blake3::hash(&payload).as_bytes());
    Ok(frame)
}

fn decode_framed_v1<T>(frame: &[u8], kind: &'static str) -> Result<T, PumpResearchRawCodecErrorV1>
where
    T: DeserializeOwned,
{
    const FRAME_OVERHEAD: usize = 4 + 32;
    if frame.len() < FRAME_OVERHEAD {
        return Err(PumpResearchRawCodecErrorV1::FrameTooShort);
    }

    let payload_len = u32::from_le_bytes(
        frame[..4]
            .try_into()
            .map_err(|_| PumpResearchRawCodecErrorV1::FrameTooShort)?,
    ) as usize;
    if payload_len > PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1 {
        return Err(PumpResearchRawCodecErrorV1::RecordTooLarge {
            kind,
            actual_bytes: payload_len,
            max_bytes: PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1,
        });
    }

    let expected_bytes = FRAME_OVERHEAD.checked_add(payload_len).ok_or(
        PumpResearchRawCodecErrorV1::InvalidFrameLength {
            actual_bytes: frame.len(),
            expected_bytes: usize::MAX,
        },
    )?;
    if frame.len() != expected_bytes {
        return Err(PumpResearchRawCodecErrorV1::InvalidFrameLength {
            actual_bytes: frame.len(),
            expected_bytes,
        });
    }

    let payload_end = 4 + payload_len;
    let payload = &frame[4..payload_end];
    let expected_hash = &frame[payload_end..];
    if blake3::hash(payload).as_bytes() != expected_hash {
        return Err(PumpResearchRawCodecErrorV1::PayloadHashMismatch);
    }

    frozen_bincode_options()
        .deserialize(payload)
        .map_err(|error| PumpResearchRawCodecErrorV1::Bincode {
            message: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    const FROZEN_DESCRIPTOR: &[u8] =
        include_bytes!("../tests/fixtures/pump_research_tape_v1/yellowstone_v1_descriptor.pb");
    const FROZEN_CORPUS_MANIFEST: &str =
        include_str!("../tests/fixtures/pump_research_tape_v1/corpus_manifest_v1.json");
    const FROZEN_GOLDEN_RECORD: &[u8] =
        include_bytes!("../tests/fixtures/pump_research_tape_v1/raw_record_v1.bin");
    const FROZEN_GOLDEN_SEGMENT: &[u8] =
        include_bytes!("../tests/fixtures/pump_research_tape_v1/raw_segment_v1.bin");
    const FROZEN_GOLDEN_RECORD_SHA256: &str =
        "5cd3df57769ba4d7024a10829c726a2a6e4fb4269eeec4a79e7d5d871d6c8334";
    const FROZEN_GOLDEN_RECORD_BLAKE3: &str =
        "8b43537b4516605fff434d919d80acd854a85e4b76928777fd08c3bee04b5846";
    const FROZEN_GOLDEN_SEGMENT_SHA256: &str =
        "c92cd25c9018680769067d57f1c5573439e7d0f139c954736c492ab994a31639";
    const FROZEN_GOLDEN_SEGMENT_BLAKE3: &str =
        "ff4007b65ff1b2c1dd5bc99cf9ccc9692b93835b42feb79ea767e4ca7dcdcf6f";

    fn bytes32(seed: u8) -> PumpResearchStorageHashV1 {
        PumpResearchStorageHashV1::new([seed; 32])
    }

    fn signature(seed: u8) -> PumpResearchStorageSignatureV1 {
        PumpResearchStorageSignatureV1::new([seed; 64])
    }

    fn source(sequence: u64, hash_seed: u8) -> PumpRawSourceEnvelopeV1 {
        PumpRawSourceEnvelopeV1 {
            provider_id: "primary-yellowstone".to_owned(),
            provider_role: PumpResearchProviderRoleV1::PrimaryAuthority,
            stream_epoch: 7,
            capture_sequence: sequence,
            payload_hash_blake3: bytes32(hash_seed),
        }
    }

    fn representative_records() -> Vec<PumpResearchRawRecordV1> {
        vec![
            PumpResearchRawRecordV1::PrimaryTransaction(PumpPrimaryTransactionEvidenceV1 {
                source: source(10, 1),
                slot: 99,
                tx_index: Some(0),
                signature: signature(2),
                event_time: PumpResearchEventTimeV1 {
                    chain_event_ts_ms: Some(1_700_000_000_000),
                    ingress_wall_ts_ms: Some(1_700_000_000_050),
                    ingress_monotonic_ts_ms: Some(50),
                },
                block_time: Some(1_700_000_000),
                source_payload: vec![1, 2, 3, 4],
            }),
            PumpResearchRawRecordV1::PrimaryAccountUpdate(PumpPrimaryAccountUpdateEvidenceV1 {
                source: source(11, 3),
                account_role: PumpResearchAccountRoleV1::BondingCurve,
                is_startup: false,
                account_pubkey: PumpResearchStoragePubkeyV1::new([4; 32]),
                owner_program: PumpResearchStoragePubkeyV1::new([5; 32]),
                raw_account_data: vec![6, 7, 8],
                raw_account_data_hash_blake3: bytes32(6),
                slot: 99,
                write_version: 12,
                txn_signature: Some(signature(7)),
                event_time: PumpResearchEventTimeV1::default(),
                source_payload: vec![9, 10],
            }),
            PumpResearchRawRecordV1::PrimarySlotUpdate(PumpPrimarySlotEvidenceV1 {
                source: source(12, 8),
                slot: 99,
                parent: Some(98),
                source_status: 2,
                event_time: PumpResearchEventTimeV1::default(),
                source_payload: vec![11],
            }),
            PumpResearchRawRecordV1::PrimaryBlockMeta(PumpPrimaryBlockMetaEvidenceV1 {
                source: source(13, 9),
                slot: 99,
                parent_slot: 98,
                block_time: Some(1_700_000_000),
                event_time: PumpResearchEventTimeV1::default(),
                source_payload: vec![12],
            }),
            PumpResearchRawRecordV1::CoverageGap(PumpRawCoverageGapV1 {
                gap_id_blake3: bytes32(10),
                provider_id: "primary-yellowstone".to_owned(),
                stream_epoch: 7,
                episode_sequence: 1,
                reason: PumpRawCoverageGapReasonV1::EvidenceQueueSaturated,
                before: PumpRawCoverageBoundaryV1 {
                    slot: Some(99),
                    signature: Some(signature(11)),
                },
                after: PumpRawCoverageBoundaryV1 {
                    slot: Some(100),
                    signature: Some(signature(12)),
                },
                missing_event_count: 3,
                first_dropped: PumpRawCoverageBoundaryV1::default(),
                last_dropped: PumpRawCoverageBoundaryV1::default(),
                queue_high_water: 2_048,
                started_at_wall_ms: 1_700_000_000_060,
                ended_at_wall_ms: 1_700_000_000_070,
                recovered: false,
            }),
            PumpResearchRawRecordV1::SegmentClosed(PumpRawSegmentClosedV1 {
                storage_format_version: PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
                segment_index: 0,
                accepted_record_count: 5,
                data_bytes: 123,
                segment_blake3: bytes32(13),
                closed_wall_ts_ms: 1_700_000_000_080,
                clean_shutdown: true,
            }),
        ]
    }

    fn representative_header() -> PumpRawSegmentHeaderV1 {
        PumpRawSegmentHeaderV1 {
            storage_format_version: PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
            run_id: "run-cs0".to_owned(),
            segment_index: 0,
            stream_epoch: 7,
            opened_wall_ts_ms: 1_700_000_000_000,
            opened_monotonic_ts_ms: 0,
            previous_segment_blake3: None,
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn source_descriptor_fixture_is_frozen_by_sha256() {
        assert_eq!(
            hex(&Sha256::digest(FROZEN_DESCRIPTOR)),
            PUMP_RESEARCH_SOURCE_DESCRIPTOR_SHA256_HEX_V1
        );
        assert_eq!(
            PUMP_RESEARCH_SOURCE_PROTO_DESCRIPTOR_HASH_V1,
            format!("sha256:{}", PUMP_RESEARCH_SOURCE_DESCRIPTOR_SHA256_HEX_V1)
        );
        assert!(FROZEN_DESCRIPTOR
            .windows(b"geyser.proto".len())
            .any(|window| window == b"geyser.proto"));
        assert!(FROZEN_DESCRIPTOR
            .windows(b"solana-storage.proto".len())
            .any(|window| window == b"solana-storage.proto"));
    }

    #[test]
    fn frozen_corpus_inventory_covers_each_cs0_contract_case_once() {
        let manifest: serde_json::Value =
            serde_json::from_str(FROZEN_CORPUS_MANIFEST).expect("frozen corpus manifest is JSON");
        assert_eq!(manifest["schema_version"], 1);

        let cases = manifest["cases"]
            .as_array()
            .expect("frozen corpus has a cases array");
        let actual = cases
            .iter()
            .map(|case| {
                case["id"]
                    .as_str()
                    .expect("each frozen corpus case has a string id")
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            actual.len(),
            cases.len(),
            "frozen corpus case identifiers must be unique"
        );

        let expected = [
            "create_v1",
            "create_v2",
            "create_plus_initial_buy",
            "single_buy",
            "single_sell",
            "multi_buy_sell_same_curve",
            "multiple_curves_one_transaction",
            "inner_cpi_trade",
            "tx_index_zero",
            "failed_transaction",
            "unknown_curve_mutation",
            "missing_final_tx_signature",
            "same_version_different_hash_account_conflict",
            "creator_not_create_user",
            "clean_process_restart_boundary",
            "processed_slot_later_rooted",
            "processed_fork_proven_dead",
            "unresolved_tail",
            "direct_top_level_pump",
            "router_to_pump_cpi",
            "v0_loaded_address_pump",
            "programdata_hash_match",
            "programdata_hash_mismatch",
            "global_startup_snapshot",
            "missing_global_predecessor",
            "canonical_ata_participant_balance",
            "non_ata_touch_only_participant_account",
            "multi_mutation_ambiguous_participant_balance",
            "frozen_binary_segment_v1",
        ]
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(actual, expected);
    }

    #[test]
    fn fixed_storage_bytes_are_exact_width_under_frozen_bincode() {
        let serialized = frozen_bincode_options()
            .serialize(&signature(42))
            .expect("signature must serialize");
        assert_eq!(serialized.len(), 64);
        assert_eq!(serialized, vec![42; 64]);
    }

    #[test]
    fn raw_record_round_trips_with_exact_frame() {
        for record in representative_records() {
            let frame = PumpResearchRawCodecV1::encode_record(&record).expect("encode record");
            assert_eq!(PumpResearchRawCodecV1::decode_record(&frame), Ok(record));
        }
    }

    #[test]
    fn raw_codec_rejects_bad_hash_and_trailing_frame_bytes() {
        let record = representative_records().remove(0);
        let frame = PumpResearchRawCodecV1::encode_record(&record).expect("encode record");

        let mut corrupt_hash = frame.clone();
        let last = corrupt_hash.len() - 1;
        corrupt_hash[last] ^= 0x01;
        assert_eq!(
            PumpResearchRawCodecV1::decode_record(&corrupt_hash),
            Err(PumpResearchRawCodecErrorV1::PayloadHashMismatch)
        );

        let mut trailing = frame;
        trailing.push(0);
        assert!(matches!(
            PumpResearchRawCodecV1::decode_record(&trailing),
            Err(PumpResearchRawCodecErrorV1::InvalidFrameLength { .. })
        ));
    }

    #[test]
    fn raw_codec_rejects_payload_above_frozen_limit() {
        let mut record = match representative_records().remove(0) {
            PumpResearchRawRecordV1::PrimaryTransaction(record) => record,
            _ => unreachable!("fixture begins with a transaction"),
        };
        record.source_payload = vec![0; PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1 + 1];
        assert!(matches!(
            PumpResearchRawCodecV1::encode_record(&PumpResearchRawRecordV1::PrimaryTransaction(
                record
            )),
            Err(PumpResearchRawCodecErrorV1::RecordTooLarge { .. })
        ));
    }

    #[test]
    fn segment_header_requires_magic_and_has_no_trailing_bytes() {
        let encoded = PumpResearchRawCodecV1::encode_segment_header(&representative_header())
            .expect("header must encode");
        assert_eq!(
            PumpResearchRawCodecV1::decode_segment_header(&encoded),
            Ok(representative_header())
        );

        let mut wrong_magic = encoded.clone();
        wrong_magic[0] ^= 0x01;
        assert_eq!(
            PumpResearchRawCodecV1::decode_segment_header(&wrong_magic),
            Err(PumpResearchRawCodecErrorV1::SegmentMagicMismatch)
        );

        let mut trailing = encoded;
        trailing.push(0);
        assert!(matches!(
            PumpResearchRawCodecV1::decode_segment_header(&trailing),
            Err(PumpResearchRawCodecErrorV1::InvalidFrameLength { .. })
        ));
    }

    #[test]
    fn frozen_golden_binary_fixtures_decode_and_reencode_identically() {
        assert_eq!(
            hex(&Sha256::digest(FROZEN_GOLDEN_RECORD)),
            FROZEN_GOLDEN_RECORD_SHA256
        );
        assert_eq!(
            blake3::hash(FROZEN_GOLDEN_RECORD).to_hex().as_str(),
            FROZEN_GOLDEN_RECORD_BLAKE3
        );
        let decoded_record = PumpResearchRawCodecV1::decode_record(FROZEN_GOLDEN_RECORD)
            .expect("current V1 decoder must read frozen record fixture");
        assert_eq!(
            PumpResearchRawCodecV1::encode_record(&decoded_record)
                .expect("decoded V1 record must re-encode"),
            FROZEN_GOLDEN_RECORD
        );

        assert_eq!(
            hex(&Sha256::digest(FROZEN_GOLDEN_SEGMENT)),
            FROZEN_GOLDEN_SEGMENT_SHA256
        );
        assert_eq!(
            blake3::hash(FROZEN_GOLDEN_SEGMENT).to_hex().as_str(),
            FROZEN_GOLDEN_SEGMENT_BLAKE3
        );

        let mut cursor = PUMP_RESEARCH_RAW_SEGMENT_MAGIC_V1.len();
        assert!(FROZEN_GOLDEN_SEGMENT.starts_with(&PUMP_RESEARCH_RAW_SEGMENT_MAGIC_V1));
        let header_frame = next_framed_fixture(FROZEN_GOLDEN_SEGMENT, &mut cursor);
        let header = decode_framed_v1::<PumpRawSegmentHeaderV1>(header_frame, "segment header")
            .expect("current V1 decoder must read frozen header fixture");
        let mut records = Vec::new();
        while cursor < FROZEN_GOLDEN_SEGMENT.len() {
            let frame = next_framed_fixture(FROZEN_GOLDEN_SEGMENT, &mut cursor);
            records.push(
                PumpResearchRawCodecV1::decode_record(frame)
                    .expect("current V1 decoder must read frozen segment record"),
            );
        }
        assert_eq!(records.len(), 6, "fixture covers every frozen raw variant");

        let mut canonical = PumpResearchRawCodecV1::encode_segment_header(&header)
            .expect("decoded V1 header must re-encode");
        for record in records {
            canonical.extend(
                PumpResearchRawCodecV1::encode_record(&record)
                    .expect("decoded V1 record must re-encode"),
            );
        }
        assert_eq!(canonical, FROZEN_GOLDEN_SEGMENT);
    }

    fn next_framed_fixture<'a>(bytes: &'a [u8], cursor: &mut usize) -> &'a [u8] {
        assert!(bytes.len().saturating_sub(*cursor) >= 4 + 32);
        let payload_len = u32::from_le_bytes(
            bytes[*cursor..*cursor + 4]
                .try_into()
                .expect("fixture contains a 4-byte frame length"),
        ) as usize;
        let end = *cursor + 4 + payload_len + 32;
        assert!(end <= bytes.len(), "fixture frame stays within segment");
        let frame = &bytes[*cursor..end];
        *cursor = end;
        frame
    }

    #[test]
    fn dependency_closure_never_requests_fee_state_for_buy_or_sell() {
        assert_eq!(
            pump_transition_dependency_v1(PumpInstructionVariantV1::BuyV2, None),
            PumpTransitionDependencyV1::None
        );
        assert_eq!(
            pump_transition_dependency_v1(PumpInstructionVariantV1::SellV2, None),
            PumpTransitionDependencyV1::None
        );
        assert_eq!(
            pump_transition_dependency_v1(
                PumpInstructionVariantV1::CreateV2,
                Some(PumpCreateInitialStateEvidenceV1::RequiresPumpGlobalFallback)
            ),
            PumpTransitionDependencyV1::PumpGlobal
        );
    }

    #[test]
    fn unresolved_slot_and_unknown_participant_are_explicit_non_successes() {
        assert_ne!(
            PumpSlotCanonicalityV1::Unresolved,
            PumpSlotCanonicalityV1::RootedCanonical
        );
        assert_eq!(
            PumpResearchWindowStatusV1::MissingRequiredEvidence(
                PumpResearchRequiredEvidenceV1::ParticipantBalance
            ),
            PumpResearchWindowStatusV1::MissingRequiredEvidence(
                PumpResearchRequiredEvidenceV1::ParticipantBalance
            )
        );
    }

    #[test]
    fn exact_manifest_raw_segment_digest_is_additive_for_historical_json() {
        let manifest = PumpExactResearchTapeManifestV1 {
            schema_version: 1,
            source_run_id: "historical-exact".to_owned(),
            source_storage_format_version: 1,
            qualification_status: PumpResearchTapeQualificationStatusV1::Unqualified,
            source_raw_segment_set_blake3: "11".repeat(32),
            go_d_source_authority: String::new(),
            external_go_e_audit_not_used_as_gate: false,
            go_d_source_authority_sha256: String::new(),
            source_descriptor_sha256: "22".repeat(32),
            program_start_receipt: PumpProgramDataReceiptV1 {
                pump_program_id: PumpResearchStoragePubkeyV1::from([1; 32]),
                pump_program_account_owner: PumpResearchStoragePubkeyV1::from([2; 32]),
                pump_programdata_pubkey: PumpResearchStoragePubkeyV1::from([3; 32]),
                program_data_owner: PumpResearchStoragePubkeyV1::from([4; 32]),
                program_data_hash_algorithm: "blake3-256".to_owned(),
                program_data_hash_blake3: PumpResearchStorageHashV1::from([5; 32]),
                program_deployment_slot: Some(6),
                observed_context_slot: 7,
                commitment: "finalized".to_owned(),
            },
            program_completion_receipt: None,
        };
        let mut historical = serde_json::to_value(&manifest).expect("serialize exact manifest");
        historical
            .as_object_mut()
            .expect("exact manifest is an object")
            .remove("source_raw_segment_set_blake3");
        let historical = historical
            .as_object_mut()
            .map(|object| {
                object.remove("go_d_source_authority");
                object.remove("external_go_e_audit_not_used_as_gate");
                object.remove("go_d_source_authority_sha256");
                serde_json::Value::Object(object.clone())
            })
            .expect("exact manifest is an object");
        let decoded: PumpExactResearchTapeManifestV1 =
            serde_json::from_value(historical).expect("decode historical exact manifest JSON");
        assert!(decoded.source_raw_segment_set_blake3.is_empty());
        assert!(decoded.go_d_source_authority.is_empty());
        assert!(!decoded.external_go_e_audit_not_used_as_gate);
        assert!(decoded.go_d_source_authority_sha256.is_empty());
    }
}
