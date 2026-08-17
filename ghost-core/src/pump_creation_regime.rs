//! Immutable, birth-level Pump creation-regime evidence.
//!
//! A Pump token's quote asset and Mayhem mode are creation facts.  They must
//! be decoded once from source-backed create evidence and inherited by later
//! strictly joined curve/mint descendants; re-inferring either property from
//! individual trades would create a second, weaker authority.

use serde::{Deserialize, Serialize};

/// Which Pump creation instruction supplied the direct birth evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PumpCreationVariantV1 {
    Create,
    CreateV2,
    #[default]
    Unknown,
}

/// Source-backed quote-asset classification for one Pump birth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PumpQuoteRegimeV1 {
    NativeSol,
    Usdc,
    Other,
    #[default]
    Unknown,
}

/// Source-backed Mayhem setting fixed when a Pump coin is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PumpMayhemModeV1 {
    True,
    False,
    #[default]
    Unknown,
}

/// Exact source shape from which the durable birth regime was materialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PumpCreationRegimeProvenanceV1 {
    /// Matching direct `create_v2` instruction and full `CreateEvent`.
    CreateV2AndCreateEvent,
    /// Full `CreateEvent` without a matching direct instruction.
    CreateEventOnly,
    /// Direct legacy `create` instruction; it carries no current quote/Mayhem
    /// contract and therefore cannot supply complete regime evidence.
    LegacyCreateInstruction,
    /// Direct `create_v2` without full matching birth-state evidence.
    CreateV2InstructionOnly,
    /// Conflicting or incomplete evidence.  Consumers must fail closed.
    #[default]
    Unknown,
}

/// Versioned birth-level regime evidence retained with `NewPoolDetected`.
///
/// `Unknown` is a first-class durable result: it means the birth cannot enter
/// a regime-sensitive offline universe, not that a consumer may guess native
/// SOL or non-Mayhem from subsequent trades.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpCreationRegimeV1 {
    pub schema_version: u16,
    pub creation_variant: PumpCreationVariantV1,
    pub quote_regime: PumpQuoteRegimeV1,
    /// Source quote-mint identity if the authoritative birth event exposed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_mint: Option<String>,
    pub mayhem_mode: PumpMayhemModeV1,
    pub provenance: PumpCreationRegimeProvenanceV1,
}

impl Default for PumpCreationRegimeV1 {
    fn default() -> Self {
        // A default must remain a durable, explicit non-evaluable fact rather
        // than schema version zero.  This preserves backward-compatible
        // deserialization while ensuring every newly emitted birth has a
        // versioned Unknown regime that an offline census can reason-code.
        Self::unknown(
            PumpCreationVariantV1::Unknown,
            PumpCreationRegimeProvenanceV1::Unknown,
        )
    }
}

impl PumpCreationRegimeV1 {
    pub const SCHEMA_VERSION: u16 = 1;

    pub fn unknown(
        creation_variant: PumpCreationVariantV1,
        provenance: PumpCreationRegimeProvenanceV1,
    ) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            creation_variant,
            quote_regime: PumpQuoteRegimeV1::Unknown,
            quote_mint: None,
            mayhem_mode: PumpMayhemModeV1::Unknown,
            provenance,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_regime_serializes_source_facts_and_defaults_to_unknown() {
        let regular = PumpCreationRegimeV1 {
            schema_version: PumpCreationRegimeV1::SCHEMA_VERSION,
            creation_variant: PumpCreationVariantV1::CreateV2,
            quote_regime: PumpQuoteRegimeV1::NativeSol,
            quote_mint: Some("11111111111111111111111111111111".to_string()),
            mayhem_mode: PumpMayhemModeV1::False,
            provenance: PumpCreationRegimeProvenanceV1::CreateV2AndCreateEvent,
        };
        let encoded = serde_json::to_value(&regular).expect("birth regime serializes");
        assert_eq!(encoded["creation_variant"], "create_v2");
        assert_eq!(encoded["quote_regime"], "native_sol");
        assert_eq!(encoded["mayhem_mode"], "false");
        assert_eq!(encoded["provenance"], "create_v2_and_create_event");

        let unknown = PumpCreationRegimeV1::default();
        assert_eq!(unknown.schema_version, PumpCreationRegimeV1::SCHEMA_VERSION);
        assert_eq!(unknown.creation_variant, PumpCreationVariantV1::Unknown);
        assert_eq!(unknown.quote_regime, PumpQuoteRegimeV1::Unknown);
        assert_eq!(unknown.mayhem_mode, PumpMayhemModeV1::Unknown);
        assert_eq!(unknown.provenance, PumpCreationRegimeProvenanceV1::Unknown);

        let instruction_only = PumpCreationRegimeV1::unknown(
            PumpCreationVariantV1::CreateV2,
            PumpCreationRegimeProvenanceV1::CreateV2InstructionOnly,
        );
        assert_eq!(
            instruction_only.creation_variant,
            PumpCreationVariantV1::CreateV2
        );
        assert_eq!(instruction_only.quote_regime, PumpQuoteRegimeV1::Unknown);
        assert_eq!(instruction_only.mayhem_mode, PumpMayhemModeV1::Unknown);
    }
}
