//! Prospective, source-lossless raw contracts for Exact-State Pump Research
//! Tape V2.
//!
//! This module is deliberately independent from the frozen GO-D V1 tape.
//! V2 exists because a future exact-state capture must preserve the bounded,
//! prospective Pump stream needed for exact-state qualification.  Its account
//! source is deliberately limited to canonical BondingCurve updates plus the
//! canonical Global account; it does not claim a historical or global
//! Pump-owned-account snapshot or global account-universe scan.
//!
//! The module remains data-only.  It does not connect to Yellowstone, make an
//! RPC request, interpret Pump instruction semantics, or become an active
//! Ghost runtime authority.

use crate::pump_research_tape::{
    PumpResearchEventTimeV1, PumpResearchStorageHashV1, PumpResearchStoragePubkeyV1,
    PumpResearchStorageSignatureV1,
};
use bincode::Options;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

/// A new V2 run never reuses the V1 storage declaration.  The two artifact
/// directories and their decoders remain intentionally disjoint.
pub const PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2: u16 = 3;

/// Magic preceding every framed V2 segment header.
pub const PUMP_EXACT_STATE_TAPE_SEGMENT_MAGIC_V2: [u8; 8] = *b"PRXTAPE3";

/// The source-record payload limit remains intentionally bounded.  V2 gains
/// coverage through its prospective source lanes, not through an unbounded
/// frame.
pub const PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2: usize = 16 * 1024 * 1024;

/// A full Yellowstone `SubscribeUpdateBlock` may be substantially larger than
/// a single transaction/account source update.  V2 retains it in bounded
/// chunks so the independent in-tape completeness evidence cannot weaken the
/// frozen per-record upper bound.
pub const PUMP_EXACT_STATE_TAPE_FULL_BLOCK_CHUNK_BYTES_V2: usize = 4 * 1024 * 1024;

/// Exact source semantics: deterministic re-encoding of the decoded
/// Yellowstone protobuf update, not a claimed original HTTP/2 wire frame.
///
/// Revision `v4` is the stream-only V1.1 contract: lossless decoded protobuf
/// records for canonical BondingCurve/Global account updates and full blocks,
/// with no account-universe snapshot or historical state repair.
pub const PUMP_EXACT_STATE_TAPE_SOURCE_CAPTURE_SEMANTICS_V2: &str =
    "decoded_protobuf_schema_lossless_bonding_curve_global_and_full_blocks_v4";

/// The one source authority admitted to a prospective V2 raw run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpExactStateProviderRoleV2 {
    PrimaryAuthority,
}

/// Per-record origin.  `capture_sequence` is allocated before bounded-ingress
/// admission, so an omitted source sequence is represented by an explicit
/// coverage gap record rather than silently disappearing. The only deliberate
/// exception is the one sequence reserved for
/// `ProspectiveStreamBoundary`, documented on that boundary type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateSourceEnvelopeV2 {
    pub provider_id: String,
    pub provider_role: PumpExactStateProviderRoleV2,
    pub stream_epoch: u64,
    pub capture_sequence: u64,
    pub payload_hash_blake3: PumpResearchStorageHashV1,
}

/// Capture-time classification is deliberately about the exact, closed
/// account-subscription scope.  A raw V3 run cannot retain an unscoped
/// Pump-owned account merely because an earlier physical format allowed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpExactStateAccountEvidenceClassV2 {
    CanonicalBondingCurve,
    CanonicalGlobal,
}

/// Source-lossless Pump transaction evidence.  The deterministic protobuf
/// payload is the complete decoded `SubscribeUpdate`, preserving its filter
/// envelope plus account keys, message header, loaded addresses, outer and
/// inner instructions, logs and transaction metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateTransactionEvidenceV2 {
    pub source: PumpExactStateSourceEnvelopeV2,
    pub slot: u64,
    pub tx_index: Option<u32>,
    pub signature: PumpResearchStorageSignatureV1,
    pub event_time: PumpResearchEventTimeV1,
    pub block_time: Option<i64>,
    pub source_payload: Vec<u8>,
}

/// Source-lossless update for one account admitted by the V2 closed
/// BondingCurve-or-canonical-Global subscription.  This is intentionally not
/// a normalized curve state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStatePumpOwnedAccountUpdateV2 {
    pub source: PumpExactStateSourceEnvelopeV2,
    pub evidence_class: PumpExactStateAccountEvidenceClassV2,
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

/// Source-lossless slot evidence retained for post-capture canonicality.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateSlotEvidenceV2 {
    pub source: PumpExactStateSourceEnvelopeV2,
    pub slot: u64,
    pub parent: Option<u64>,
    pub source_status: i32,
    pub event_time: PumpResearchEventTimeV1,
    pub source_payload: Vec<u8>,
}

/// Source-lossless block metadata retained for per-slot full-block
/// reconciliation, stream-boundary and time proof.  These identity fields
/// are deliberately duplicated from the matching full-block payload: two
/// lanes agreeing only on their observed Pump transactions would not prove
/// that either lane was complete.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateBlockMetaEvidenceV2 {
    pub source: PumpExactStateSourceEnvelopeV2,
    pub slot: u64,
    pub parent_slot: u64,
    pub blockhash: String,
    pub parent_blockhash: String,
    pub executed_transaction_count: u64,
    pub block_time: Option<i64>,
    pub event_time: PumpResearchEventTimeV1,
    pub source_payload: Vec<u8>,
}

/// First record for an unfiltered block payload retained alongside the
/// Pump-filtered transaction stream.  The block's complete transaction list
/// is source evidence used later to prove that every Pump invocation found in
/// a captured canonical block also has a corresponding filtered transaction
/// record.  It is not an RPC repair or a second provider.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateFullBlockPayloadStartedV2 {
    pub source: PumpExactStateSourceEnvelopeV2,
    pub slot: u64,
    pub parent_slot: u64,
    pub blockhash: String,
    pub parent_blockhash: String,
    pub executed_transaction_count: u64,
    /// Timestamp from the same source update as the complete block payload.
    /// The offline qualifier uses the monotonic half for duration/cutoff
    /// authority and retains the wall-clock half solely as an audit label.
    pub event_time: PumpResearchEventTimeV1,
    pub source_payload_sha256: PumpResearchStorageHashV1,
    pub source_payload_bytes: u64,
    pub source_payload_chunk_count: u64,
}

/// One ordered source-lossless chunk from the complete decoded
/// `SubscribeUpdate` whose inner variant is a full block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateFullBlockPayloadChunkV2 {
    pub source_capture_sequence: u64,
    pub chunk_index: u64,
    pub bytes: Vec<u8>,
}

/// Terminal receipt for the same full block source payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateFullBlockPayloadCompletedV2 {
    pub source_capture_sequence: u64,
    pub source_payload_blake3: PumpResearchStorageHashV1,
    pub source_payload_sha256: PumpResearchStorageHashV1,
    pub source_payload_bytes: u64,
    pub source_payload_chunk_count: u64,
}

/// The first admitted slot from each source lane required to establish a
/// prospective Exact-State V2 cohort.  These are *source-start* facts, not
/// canonicality claims: a later offline qualifier still derives canonical
/// slots from the frozen Slot/BlockMeta/full-block evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateSourceReadinessV2 {
    pub first_transaction_slot: u64,
    pub first_account_update_slot: u64,
    pub first_slot_update_slot: u64,
    pub first_block_meta_slot: u64,
    pub first_full_block_slot: u64,
    pub source_readiness_slot: u64,
}

/// The only boundary at which an offline V2 materializer may begin a
/// prospective cohort.  It is emitted exactly once after all five source
/// lanes have admitted one record and the writer has durably flushed the
/// boundary in the same raw segment chain as the continuous source stream.
/// A qualifier must treat every candidate slot at or below
/// `cohort_slots_strictly_after` as warm-up evidence, never as a
/// prospectively complete outcome cohort.
///
/// `source_capture_sequence_exclusive` is diagnostic provenance for the
/// concurrent stream. Its numeric position is reserved for the durable
/// boundary control record itself: source records below it are warm-up, no
/// source record receives that marker, and later source records have strictly
/// larger values. Slot/canonicality evidence, not this concurrent counter
/// alone, remains the authority for later replay ordering.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateProspectiveStreamBoundaryV2 {
    pub source_readiness: PumpExactStateSourceReadinessV2,
    pub source_stream_epoch: u64,
    pub source_capture_sequence_exclusive: u64,
    pub cohort_slots_strictly_after: u64,
    pub sealed_wall_ts_ms: u64,
    pub sealed_monotonic_ts_ms: u64,
}

/// Fixed-width boundary around one locally observed coverage discontinuity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateCoverageBoundaryV2 {
    pub slot: Option<u64>,
    pub signature: Option<PumpResearchStorageSignatureV1>,
}

/// A typed reason why V2 could not admit a source update.  Any retained gap is
/// a qualification blocker; it is never repaired from RPC or a later state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpExactStateCoverageGapReasonV2 {
    IngressQueueSaturated,
    WalQueueSaturated,
    EvidenceQueueSaturated,
    IpcEgressQueueSaturated,
    RecordExceedsFrozenLimit,
}

/// One continuous local coverage gap episode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateCoverageGapV2 {
    pub gap_id_blake3: PumpResearchStorageHashV1,
    pub provider_id: String,
    pub stream_epoch: u64,
    pub episode_sequence: u64,
    pub reason: PumpExactStateCoverageGapReasonV2,
    pub before: PumpExactStateCoverageBoundaryV2,
    pub after: PumpExactStateCoverageBoundaryV2,
    pub missing_event_count: u64,
    pub first_dropped: PumpExactStateCoverageBoundaryV2,
    pub last_dropped: PumpExactStateCoverageBoundaryV2,
    pub queue_high_water: u64,
    pub started_at_wall_ms: u64,
    pub ended_at_wall_ms: u64,
    pub recovered: bool,
}

/// Atomic segment footer.  The `segment_blake3` covers the header and every
/// non-footer frame exactly as written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateSegmentClosedV2 {
    pub storage_format_version: u16,
    pub segment_index: u64,
    pub accepted_record_count: u64,
    pub data_bytes: u64,
    pub segment_blake3: PumpResearchStorageHashV1,
    pub closed_wall_ts_ms: u64,
    pub clean_shutdown: bool,
}

/// The only V3 raw record declaration.  Its variant order is V3 physical
/// storage layout and must be treated as frozen once the first V3 run is
/// admitted.  There is deliberately no account-snapshot record family.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PumpExactStateRawRecordV2 {
    PrimaryTransaction(PumpExactStateTransactionEvidenceV2),
    PumpOwnedAccountUpdate(PumpExactStatePumpOwnedAccountUpdateV2),
    PrimarySlotUpdate(PumpExactStateSlotEvidenceV2),
    PrimaryBlockMeta(PumpExactStateBlockMetaEvidenceV2),
    FullBlockPayloadStarted(PumpExactStateFullBlockPayloadStartedV2),
    FullBlockPayloadChunk(PumpExactStateFullBlockPayloadChunkV2),
    FullBlockPayloadCompleted(PumpExactStateFullBlockPayloadCompletedV2),
    ProspectiveStreamBoundary(PumpExactStateProspectiveStreamBoundaryV2),
    CoverageGap(PumpExactStateCoverageGapV2),
    SegmentClosed(PumpExactStateSegmentClosedV2),
}

/// Header of one V2 segment.  The `capture_contract_sha256` binds the segment
/// to the sealed V2 config and semantics contract selected before source I/O.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateSegmentHeaderV2 {
    pub storage_format_version: u16,
    pub run_id: String,
    pub segment_index: u64,
    pub stream_epoch: u64,
    pub opened_wall_ts_ms: u64,
    pub opened_monotonic_ts_ms: u64,
    pub capture_contract_sha256: PumpResearchStorageHashV1,
    pub previous_segment_blake3: Option<PumpResearchStorageHashV1>,
}

/// V2 framing errors remain typed so a corrupt or mis-versioned run cannot be
/// decoded as a valid prospective exact-state tape.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PumpExactStateRawCodecErrorV2 {
    #[error("V2 raw {kind} payload is {actual_bytes} bytes, above frozen {max_bytes}-byte limit")]
    RecordTooLarge {
        kind: &'static str,
        actual_bytes: usize,
        max_bytes: usize,
    },
    #[error("V2 raw frame is {actual_bytes} bytes; expected exactly {expected_bytes}")]
    InvalidFrameLength {
        actual_bytes: usize,
        expected_bytes: usize,
    },
    #[error("V2 raw frame is shorter than its mandatory 4-byte length and 32-byte digest")]
    FrameTooShort,
    #[error("V2 raw payload digest mismatch")]
    PayloadHashMismatch,
    #[error("V2 raw segment magic mismatch")]
    SegmentMagicMismatch,
    #[error("V2 segment header declares storage format {actual}, expected {expected}")]
    StorageFormatVersionMismatch { actual: u16, expected: u16 },
    #[error("V2 bincode failure: {message}")]
    Bincode { message: String },
}

/// Framed V2 codec:
/// `u32 little-endian payload_length | bincode fixed-int little-endian payload
/// | BLAKE3-256(payload)`.
pub struct PumpExactStateRawCodecV2;

impl PumpExactStateRawCodecV2 {
    #[must_use]
    pub const fn storage_format_version() -> u16 {
        PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2
    }

    pub fn encode_record(
        record: &PumpExactStateRawRecordV2,
    ) -> Result<Vec<u8>, PumpExactStateRawCodecErrorV2> {
        encode_framed_v2(record, "record")
    }

    pub fn decode_record(
        frame: &[u8],
    ) -> Result<PumpExactStateRawRecordV2, PumpExactStateRawCodecErrorV2> {
        decode_framed_v2(frame, "record")
    }

    pub fn encode_segment_header(
        header: &PumpExactStateSegmentHeaderV2,
    ) -> Result<Vec<u8>, PumpExactStateRawCodecErrorV2> {
        if header.storage_format_version != PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2 {
            return Err(
                PumpExactStateRawCodecErrorV2::StorageFormatVersionMismatch {
                    actual: header.storage_format_version,
                    expected: PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
                },
            );
        }
        let frame = encode_framed_v2(header, "segment header")?;
        let mut encoded =
            Vec::with_capacity(PUMP_EXACT_STATE_TAPE_SEGMENT_MAGIC_V2.len() + frame.len());
        encoded.extend_from_slice(&PUMP_EXACT_STATE_TAPE_SEGMENT_MAGIC_V2);
        encoded.extend_from_slice(&frame);
        Ok(encoded)
    }

    pub fn decode_segment_header(
        encoded: &[u8],
    ) -> Result<PumpExactStateSegmentHeaderV2, PumpExactStateRawCodecErrorV2> {
        if !encoded.starts_with(&PUMP_EXACT_STATE_TAPE_SEGMENT_MAGIC_V2) {
            return Err(PumpExactStateRawCodecErrorV2::SegmentMagicMismatch);
        }
        let header: PumpExactStateSegmentHeaderV2 = decode_framed_v2(
            &encoded[PUMP_EXACT_STATE_TAPE_SEGMENT_MAGIC_V2.len()..],
            "segment header",
        )?;
        if header.storage_format_version != PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2 {
            return Err(
                PumpExactStateRawCodecErrorV2::StorageFormatVersionMismatch {
                    actual: header.storage_format_version,
                    expected: PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
                },
            );
        }
        Ok(header)
    }
}

fn v2_bincode_options() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
}

fn encode_framed_v2<T>(
    value: &T,
    kind: &'static str,
) -> Result<Vec<u8>, PumpExactStateRawCodecErrorV2>
where
    T: Serialize,
{
    let payload = v2_bincode_options().serialize(value).map_err(|error| {
        PumpExactStateRawCodecErrorV2::Bincode {
            message: error.to_string(),
        }
    })?;
    if payload.len() > PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2 {
        return Err(PumpExactStateRawCodecErrorV2::RecordTooLarge {
            kind,
            actual_bytes: payload.len(),
            max_bytes: PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2,
        });
    }
    let payload_len = u32::try_from(payload.len()).map_err(|_| {
        PumpExactStateRawCodecErrorV2::RecordTooLarge {
            kind,
            actual_bytes: payload.len(),
            max_bytes: PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2,
        }
    })?;
    let mut frame = Vec::with_capacity(4 + payload.len() + 32);
    frame.extend_from_slice(&payload_len.to_le_bytes());
    frame.extend_from_slice(&payload);
    frame.extend_from_slice(blake3::hash(&payload).as_bytes());
    Ok(frame)
}

fn decode_framed_v2<T>(frame: &[u8], kind: &'static str) -> Result<T, PumpExactStateRawCodecErrorV2>
where
    T: DeserializeOwned,
{
    const FRAME_OVERHEAD: usize = 4 + 32;
    if frame.len() < FRAME_OVERHEAD {
        return Err(PumpExactStateRawCodecErrorV2::FrameTooShort);
    }
    let payload_len = u32::from_le_bytes(
        frame[..4]
            .try_into()
            .map_err(|_| PumpExactStateRawCodecErrorV2::FrameTooShort)?,
    ) as usize;
    if payload_len > PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2 {
        return Err(PumpExactStateRawCodecErrorV2::RecordTooLarge {
            kind,
            actual_bytes: payload_len,
            max_bytes: PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2,
        });
    }
    let expected_bytes = FRAME_OVERHEAD.checked_add(payload_len).ok_or(
        PumpExactStateRawCodecErrorV2::InvalidFrameLength {
            actual_bytes: frame.len(),
            expected_bytes: usize::MAX,
        },
    )?;
    if frame.len() != expected_bytes {
        return Err(PumpExactStateRawCodecErrorV2::InvalidFrameLength {
            actual_bytes: frame.len(),
            expected_bytes,
        });
    }
    let payload_end = 4 + payload_len;
    let payload = &frame[4..payload_end];
    if blake3::hash(payload).as_bytes() != &frame[payload_end..] {
        return Err(PumpExactStateRawCodecErrorV2::PayloadHashMismatch);
    }
    v2_bincode_options().deserialize(payload).map_err(|error| {
        PumpExactStateRawCodecErrorV2::Bincode {
            message: error.to_string(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pump_research_tape::PumpResearchFixedBytesV1;

    fn source(sequence: u64) -> PumpExactStateSourceEnvelopeV2 {
        PumpExactStateSourceEnvelopeV2 {
            provider_id: "primary-test".to_owned(),
            provider_role: PumpExactStateProviderRoleV2::PrimaryAuthority,
            stream_epoch: 1,
            capture_sequence: sequence,
            payload_hash_blake3: PumpResearchFixedBytesV1::new([7; 32]),
        }
    }

    fn representative_record() -> PumpExactStateRawRecordV2 {
        PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(PumpExactStatePumpOwnedAccountUpdateV2 {
            source: source(4),
            evidence_class: PumpExactStateAccountEvidenceClassV2::CanonicalBondingCurve,
            is_startup: false,
            account_pubkey: PumpResearchFixedBytesV1::new([1; 32]),
            owner_program: PumpResearchFixedBytesV1::new([2; 32]),
            raw_account_data: vec![3, 4, 5],
            raw_account_data_hash_blake3: PumpResearchFixedBytesV1::new([6; 32]),
            slot: 77,
            write_version: 91,
            txn_signature: Some(PumpResearchFixedBytesV1::new([8; 64])),
            event_time: PumpResearchEventTimeV1 {
                chain_event_ts_ms: Some(1_700_000_000_000),
                ingress_wall_ts_ms: Some(1_700_000_000_001),
                ingress_monotonic_ts_ms: Some(42),
            },
            source_payload: vec![9, 10],
        })
    }

    fn representative_header() -> PumpExactStateSegmentHeaderV2 {
        PumpExactStateSegmentHeaderV2 {
            storage_format_version: PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
            run_id: "prospective-v2-test".to_owned(),
            segment_index: 0,
            stream_epoch: 1,
            opened_wall_ts_ms: 1_700_000_000_000,
            opened_monotonic_ts_ms: 42,
            capture_contract_sha256: PumpResearchFixedBytesV1::new([11; 32]),
            previous_segment_blake3: None,
        }
    }

    #[test]
    fn v3_round_trips_closed_scope_bonding_curve_account() {
        let record = representative_record();
        let frame = PumpExactStateRawCodecV2::encode_record(&record).expect("encode V2 record");
        assert_eq!(PumpExactStateRawCodecV2::decode_record(&frame), Ok(record));
    }

    #[test]
    fn v2_rejects_payload_hash_drift() {
        let mut frame = PumpExactStateRawCodecV2::encode_record(&representative_record())
            .expect("encode V2 record");
        let last = frame.len() - 1;
        frame[last] ^= 0x01;
        assert_eq!(
            PumpExactStateRawCodecV2::decode_record(&frame),
            Err(PumpExactStateRawCodecErrorV2::PayloadHashMismatch)
        );
    }

    #[test]
    fn v2_header_binds_magic_and_storage_version() {
        let header = representative_header();
        let encoded =
            PumpExactStateRawCodecV2::encode_segment_header(&header).expect("encode V2 header");
        assert_eq!(
            PumpExactStateRawCodecV2::decode_segment_header(&encoded),
            Ok(header)
        );

        let mut wrong_magic = encoded.clone();
        wrong_magic[0] ^= 0x01;
        assert_eq!(
            PumpExactStateRawCodecV2::decode_segment_header(&wrong_magic),
            Err(PumpExactStateRawCodecErrorV2::SegmentMagicMismatch)
        );

        let wrong_version = PumpExactStateSegmentHeaderV2 {
            storage_format_version: 1,
            ..representative_header()
        };
        assert_eq!(
            PumpExactStateRawCodecV2::encode_segment_header(&wrong_version),
            Err(
                PumpExactStateRawCodecErrorV2::StorageFormatVersionMismatch {
                    actual: 1,
                    expected: PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
                }
            )
        );
    }

    #[test]
    fn v3_round_trips_stream_boundary_with_all_required_lane_slots() {
        let readiness = PumpExactStateSourceReadinessV2 {
            first_transaction_slot: 101,
            first_account_update_slot: 102,
            first_slot_update_slot: 103,
            first_block_meta_slot: 104,
            first_full_block_slot: 105,
            source_readiness_slot: 105,
        };
        let record = PumpExactStateRawRecordV2::ProspectiveStreamBoundary(
            PumpExactStateProspectiveStreamBoundaryV2 {
                source_readiness: readiness.clone(),
                source_stream_epoch: 1,
                source_capture_sequence_exclusive: 42,
                cohort_slots_strictly_after: 105,
                sealed_wall_ts_ms: 1_700_000_000_000,
                sealed_monotonic_ts_ms: 77,
            },
        );
        let frame = PumpExactStateRawCodecV2::encode_record(&record)
            .expect("encode readiness boundary with complete source readiness evidence");
        assert_eq!(
            PumpExactStateRawCodecV2::decode_record(&frame),
            Ok(record),
            "every lane slot is frozen stream-only V3 raw evidence"
        );
    }

    #[test]
    fn v2_round_trips_full_block_chunks_under_the_frozen_record_bound() {
        assert!(
            PUMP_EXACT_STATE_TAPE_FULL_BLOCK_CHUNK_BYTES_V2
                < PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2,
            "full-block chunks must leave framing headroom below the raw record maximum"
        );
        let record = PumpExactStateRawRecordV2::FullBlockPayloadChunk(
            PumpExactStateFullBlockPayloadChunkV2 {
                source_capture_sequence: 19,
                chunk_index: 2,
                bytes: vec![5; 1_024],
            },
        );
        let encoded = PumpExactStateRawCodecV2::encode_record(&record)
            .expect("encode bounded full-block source chunk");
        assert!(encoded.len() < PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2);
        assert_eq!(
            PumpExactStateRawCodecV2::decode_record(&encoded),
            Ok(record),
            "full block evidence must survive the V2 frozen codec"
        );
    }
}
