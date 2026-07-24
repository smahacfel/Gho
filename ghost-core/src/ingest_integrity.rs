//! Additive contracts for the ingest observation boundary.
//!
//! These types deliberately separate structural identity, canonical chain
//! ordering, semantic claims and provider provenance.  They are data contracts
//! only: introducing them must not change the current runtime authority or
//! canonical emission path.

use crate::{ProgramFeeCharge, PumpRouteVariant};
use serde::{Deserialize, Serialize};
use solana_sdk::{pubkey::Pubkey, signature::Signature};

/// Configured role of a raw Yellowstone provider.
///
/// The role is carried as observation metadata in PR 1A.  Runtime arbitration
/// remains unchanged until the dedicated arbiter work in later PR 1 commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawProviderRoleV1 {
    PrimaryAuthority,
    SecondaryWitness,
}

/// Local (post-provider) loss domain. These reasons must never be reported as
/// Yellowstone/provider slot gaps and must never trigger a provider reconnect
/// by themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalCoverageGapReasonV1 {
    IngressQueueSaturated,
    WalQueueSaturated,
    EvidenceQueueSaturated,
    IpcEgressQueueSaturated,
}

impl LocalCoverageGapReasonV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IngressQueueSaturated => "ingress_queue_saturated",
            Self::WalQueueSaturated => "wal_queue_saturated",
            Self::EvidenceQueueSaturated => "evidence_queue_saturated",
            Self::IpcEgressQueueSaturated => "ipc_egress_queue_saturated",
        }
    }
}

/// Boundary of one deterministic local coverage gap.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalCoverageBoundaryV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<Signature>,
}

/// One continuous local saturation episode.
///
/// `gap_id_blake3` is computed from stable episode inputs (provider, stream
/// epoch, reason, sequence and boundary identities), never from a random UUID
/// or diagnostic wall-clock fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalCoverageGapV1 {
    pub gap_id_blake3: [u8; 32],
    pub provider_id: String,
    pub stream_epoch: u64,
    pub episode_sequence: u64,
    pub reason: LocalCoverageGapReasonV1,
    #[serde(default)]
    pub before: LocalCoverageBoundaryV1,
    #[serde(default)]
    pub after: LocalCoverageBoundaryV1,
    pub queue_high_water: usize,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    /// PR1B has no proof-based local replay mechanism. A closed episode is
    /// therefore still non-evaluable until an explicit later recovery contract.
    pub recovered: bool,
}

impl RawProviderRoleV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PrimaryAuthority => "primary_authority",
            Self::SecondaryWitness => "secondary_witness",
        }
    }
}

/// Observation family of an ingest record.
///
/// This deliberately does not reuse the transport-level `SourceKind`: it
/// classifies the evidence family (`raw Yellowstone` versus a parsed NLN
/// witness), not the socket, adapter, or provider transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSourceFamilyV1 {
    RawYellowstone,
    ParsedNln,
}

/// Source-neutral identity of one Pump mutation inside a transaction.
///
/// Provider identity, arrival time, slot, transaction index and semantic
/// claims are intentionally absent.  Those dimensions belong to provenance,
/// ordering or payload comparison and therefore cannot create a second
/// canonical mutation identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RawPumpMutationLocatorV1 {
    pub program_id: Pubkey,
    pub signature: Signature,
    pub outer_instruction_index: u16,
    pub inner_instruction_path: Vec<u16>,
    /// Stable ordinal of the semantic mutation inside the transaction.
    ///
    /// This matches the existing `TradeEvent::event_ordinal` width.  The
    /// locator must not introduce a narrower overflow contract than the
    /// parser-facing event model.
    pub semantic_event_ordinal: u32,
}

/// Canonical chain order supplied by primary raw Yellowstone evidence.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CanonicalPumpOrderKeyV1 {
    pub slot: u64,
    pub tx_index: u32,
    pub outer_instruction_index: u16,
    pub inner_instruction_path: Vec<u16>,
    pub semantic_event_ordinal: u32,
}

/// Typed Pump trade direction used by the semantic observation payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpTradeSideV1 {
    Buy,
    Sell,
}

/// Typed instruction-level constraint observed in a Pump instruction.
///
/// This is an observation contract only.  It does not become quote or
/// execution authority in PR 1A.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpInstructionLimitV1 {
    MaxWalletDebitLamports(u64),
    MinWalletCreditLamports(u64),
    ExactQuoteInputLamports(u64),
    MinTokenOutputUnits(u64),
}

/// Provider-reported semantic claims for one Pump mutation.
///
/// `None` means that this provider did not know or did not report a field. It
/// does not mean a default value and does not itself form a provider conflict.
/// A conflict exists only when two concrete claims disagree.  A future
/// transaction-local validator may promote a complete primary-raw observation
/// into a separate strict fact type; PR 1A intentionally does not do that.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpMutationClaimsV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Pubkey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mint: Option<Pubkey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_variant: Option<PumpRouteVariant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<PumpTradeSideV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_amount_units: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_limit: Option<PumpInstructionLimitV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_curve_quote_lamports: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_wallet_delta_lamports: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_fee_breakdown: Option<Vec<ProgramFeeCharge>>,
}

/// Provenance of one provider observation.
///
/// `received_at_monotonic_ns` is diagnostic transport metadata.  Callers must
/// not include it in locator identity, canonical payload hashes, state hashes,
/// MFS hashes or differential parity checksums.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationProvenanceV1 {
    #[serde(alias = "source_kind")]
    pub source_family: ObservationSourceFamilyV1,
    pub source_id: String,
    pub provider_id: String,
    pub schema_id: String,
    /// BLAKE3 of captured provider payload bytes handed to normalization.
    ///
    /// `source_family` and `schema_id` define the captured representation. For
    /// Yellowstone this is the prost encoding of the decoded
    /// `SubscribeUpdateTransaction` passed from the adapter to normalization;
    /// it is not an original gRPC wire frame and need not retain envelope or
    /// unknown-field bytes. The hash identifies one provider observation for
    /// audit; it is not a semantic-equivalence hash across raw protobuf and
    /// parsed JSON.
    #[serde(alias = "payload_hash")]
    pub payload_hash_blake3: [u8; 32],
    pub received_at_monotonic_ns: u64,
}

impl ObservationProvenanceV1 {
    /// Compute the contract hash for captured provider payload bytes.
    #[must_use]
    pub fn payload_hash_for_captured_provider_payload(
        captured_provider_payload: &[u8],
    ) -> [u8; 32] {
        *blake3::hash(captured_provider_payload).as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locator(signature: Signature, ordinal: u32) -> RawPumpMutationLocatorV1 {
        RawPumpMutationLocatorV1 {
            program_id: Pubkey::new_unique(),
            signature,
            outer_instruction_index: 3,
            inner_instruction_path: vec![1, 2],
            semantic_event_ordinal: ordinal,
        }
    }

    #[test]
    fn locator_is_source_neutral_and_semantics_are_not_identity() {
        let locator = locator(Signature::new_unique(), 0);
        let primary = ObservationProvenanceV1 {
            source_family: ObservationSourceFamilyV1::RawYellowstone,
            source_id: "grpc_global_stream".to_string(),
            provider_id: "provider-a".to_string(),
            schema_id: "yellowstone-1.14".to_string(),
            payload_hash_blake3: [1; 32],
            received_at_monotonic_ns: 10,
        };
        let secondary = ObservationProvenanceV1 {
            provider_id: "provider-b".to_string(),
            payload_hash_blake3: [2; 32],
            received_at_monotonic_ns: 20,
            ..primary.clone()
        };
        let payload_a = PumpMutationClaimsV1 {
            curve: Some(Pubkey::new_unique()),
            mint: Some(Pubkey::new_unique()),
            route_variant: Some(PumpRouteVariant::BuyV2),
            side: Some(PumpTradeSideV1::Buy),
            success: Some(true),
            token_amount_units: Some(100),
            instruction_limit: Some(PumpInstructionLimitV1::MaxWalletDebitLamports(200)),
            reported_curve_quote_lamports: Some(150),
            reported_wallet_delta_lamports: Some(175),
            reported_fee_breakdown: None,
        };
        let payload_b = PumpMutationClaimsV1 {
            curve: Some(Pubkey::new_unique()),
            mint: Some(Pubkey::new_unique()),
            side: Some(PumpTradeSideV1::Sell),
            ..payload_a.clone()
        };

        assert_ne!(primary, secondary);
        assert_ne!(payload_a, payload_b);
        let encoded = serde_json::to_value(&locator).expect("serialize locator");
        let object = encoded.as_object().expect("locator must be an object");
        assert_eq!(object.len(), 5);
        for identity_field in [
            "program_id",
            "signature",
            "outer_instruction_index",
            "inner_instruction_path",
            "semantic_event_ordinal",
        ] {
            assert!(object.contains_key(identity_field));
        }
        for non_identity_field in [
            "source_family",
            "source_id",
            "provider_id",
            "slot",
            "tx_index",
            "curve",
            "mint",
            "side",
            "received_at_monotonic_ns",
        ] {
            assert!(!object.contains_key(non_identity_field));
        }
    }

    #[test]
    fn order_preserves_zero_transaction_index() {
        let key = CanonicalPumpOrderKeyV1 {
            slot: 42,
            tx_index: 0,
            outer_instruction_index: 1,
            inner_instruction_path: Vec::new(),
            semantic_event_ordinal: 0,
        };

        let json = serde_json::to_string(&key).expect("serialize order key");
        assert!(json.contains("\"tx_index\":0"));
        assert_eq!(
            serde_json::from_str::<CanonicalPumpOrderKeyV1>(&json).expect("deserialize order key"),
            key
        );
    }

    #[test]
    fn one_signature_can_identify_multiple_mutations() {
        let signature = Signature::new_unique();
        let first = locator(signature, 0);
        let second = RawPumpMutationLocatorV1 {
            semantic_event_ordinal: 1,
            ..first.clone()
        };

        assert_ne!(first, second);
    }

    #[test]
    fn claims_preserve_unknown_without_inventing_defaults() {
        let claims = PumpMutationClaimsV1 {
            mint: Some(Pubkey::new_unique()),
            ..PumpMutationClaimsV1::default()
        };

        assert_eq!(claims.curve, None);
        assert_eq!(claims.route_variant, None);
        assert_eq!(claims.side, None);
        assert_eq!(claims.success, None);
        assert_eq!(claims.token_amount_units, None);

        let json = serde_json::to_value(&claims).expect("serialize optional claims");
        let object = json
            .as_object()
            .expect("claims must serialize as an object");
        assert!(object.contains_key("mint"));
        assert!(!object.contains_key("curve"));
        assert!(!object.contains_key("route_variant"));
        assert!(!object.contains_key("success"));
    }

    #[test]
    fn fully_reported_claims_round_trip_without_becoming_a_strict_fact() {
        let claims = PumpMutationClaimsV1 {
            curve: Some(Pubkey::new_unique()),
            mint: Some(Pubkey::new_unique()),
            route_variant: Some(PumpRouteVariant::BuyV2),
            side: Some(PumpTradeSideV1::Buy),
            success: Some(true),
            token_amount_units: Some(42),
            instruction_limit: Some(PumpInstructionLimitV1::ExactQuoteInputLamports(99)),
            reported_curve_quote_lamports: Some(98),
            reported_wallet_delta_lamports: Some(100),
            reported_fee_breakdown: Some(Vec::new()),
        };

        let json = serde_json::to_value(&claims).expect("serialize complete claims");
        let decoded: PumpMutationClaimsV1 =
            serde_json::from_value(json).expect("deserialize complete optional claims");

        assert_eq!(decoded, claims);
    }

    #[test]
    fn payload_hash_is_blake3_of_captured_provider_payload_bytes() {
        let bytes = b"captured-provider-payload";
        assert_eq!(
            ObservationProvenanceV1::payload_hash_for_captured_provider_payload(bytes),
            *blake3::hash(bytes).as_bytes()
        );
    }

    #[test]
    fn previous_pr1a_provenance_field_names_remain_readable() {
        let current = ObservationProvenanceV1 {
            source_family: ObservationSourceFamilyV1::RawYellowstone,
            source_id: "grpc_global_stream".to_owned(),
            provider_id: "primary".to_owned(),
            schema_id: "yellowstone-1.14".to_owned(),
            payload_hash_blake3: [7; 32],
            received_at_monotonic_ns: 42,
        };
        let mut old_shape = serde_json::to_value(&current).expect("serialize provenance");
        let object = old_shape
            .as_object_mut()
            .expect("provenance must serialize as an object");
        let source_family = object
            .remove("source_family")
            .expect("new source field must be present");
        let payload_hash_blake3 = object
            .remove("payload_hash_blake3")
            .expect("new hash field must be present");
        object.insert("source_kind".to_owned(), source_family);
        object.insert("payload_hash".to_owned(), payload_hash_blake3);

        let decoded: ObservationProvenanceV1 =
            serde_json::from_value(old_shape).expect("previous PR1A field names must deserialize");
        assert_eq!(decoded, current);
    }

    #[test]
    fn semantic_event_ordinal_is_not_narrowed_to_u16() {
        let locator = locator(Signature::new_unique(), u16::MAX as u32 + 1);
        let encoded = serde_json::to_string(&locator).expect("serialize wide ordinal");
        let decoded: RawPumpMutationLocatorV1 =
            serde_json::from_str(&encoded).expect("deserialize wide ordinal");

        assert_eq!(decoded.semantic_event_ordinal, u16::MAX as u32 + 1);
    }
}
