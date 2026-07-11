use super::CanonicalU64StringV1;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MetricIdentityErrorV1 {
    #[error("record identity field {0} must not be blank")]
    BlankRecordField(&'static str),
    #[error("stable event identity field {0} must not be blank")]
    BlankStableEventField(&'static str),
}

/// Durable record identity. Duplicate-record detection uses the complete tuple
/// `(run_id, join_key, decision_plane)` and never `join_key` alone.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct MetricEvidenceRecordIdentityV1 {
    pub run_id: String,
    pub join_key: String,
    pub decision_plane: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMetricEvidenceRecordIdentityV1 {
    run_id: String,
    join_key: String,
    decision_plane: String,
}

impl MetricEvidenceRecordIdentityV1 {
    pub fn try_new(
        run_id: impl Into<String>,
        join_key: impl Into<String>,
        decision_plane: impl Into<String>,
    ) -> Result<Self, MetricIdentityErrorV1> {
        let identity = Self {
            run_id: run_id.into(),
            join_key: join_key.into(),
            decision_plane: decision_plane.into(),
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), MetricIdentityErrorV1> {
        for (name, value) in [
            ("run_id", self.run_id.as_str()),
            ("join_key", self.join_key.as_str()),
            ("decision_plane", self.decision_plane.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(MetricIdentityErrorV1::BlankRecordField(name));
            }
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for MetricEvidenceRecordIdentityV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawMetricEvidenceRecordIdentityV1::deserialize(deserializer)?;
        Self::try_new(raw.run_id, raw.join_key, raw.decision_plane)
            .map_err(serde::de::Error::custom)
    }
}

/// Canonical key of an underlying source event. A signature is preferred. The
/// slot/order forms are explicit fallbacks and may be used only when the source
/// contract proves the selected coordinate is unique within that slot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StableEventKeyV1 {
    Signature {
        signature: String,
    },
    SlotTransactionIndex {
        slot: CanonicalU64StringV1,
        transaction_index: u32,
    },
    SlotEventOrdinal {
        slot: CanonicalU64StringV1,
        event_ordinal: u32,
    },
}

/// Stable identity of the underlying source event, distinct from record
/// identity. The whole value is optional in evidence because historical
/// streams may expose neither a signature nor a provably unique order key;
/// absence must be reported as unavailable/not-evaluable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct StableEventIdentityV1 {
    pub source: String,
    pub key: StableEventKeyV1,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStableEventIdentityV1 {
    source: String,
    key: StableEventKeyV1,
}

impl StableEventIdentityV1 {
    pub fn try_from_signature(
        source: impl Into<String>,
        signature: impl Into<String>,
    ) -> Result<Self, MetricIdentityErrorV1> {
        let identity = Self {
            source: source.into(),
            key: StableEventKeyV1::Signature {
                signature: signature.into(),
            },
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn try_from_transaction_index(
        source: impl Into<String>,
        slot: u64,
        transaction_index: u32,
    ) -> Result<Self, MetricIdentityErrorV1> {
        let identity = Self {
            source: source.into(),
            key: StableEventKeyV1::SlotTransactionIndex {
                slot: CanonicalU64StringV1::new(slot),
                transaction_index,
            },
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn try_from_event_ordinal(
        source: impl Into<String>,
        slot: u64,
        event_ordinal: u32,
    ) -> Result<Self, MetricIdentityErrorV1> {
        let identity = Self {
            source: source.into(),
            key: StableEventKeyV1::SlotEventOrdinal {
                slot: CanonicalU64StringV1::new(slot),
                event_ordinal,
            },
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), MetricIdentityErrorV1> {
        if self.source.trim().is_empty() {
            return Err(MetricIdentityErrorV1::BlankStableEventField("source"));
        }
        if let StableEventKeyV1::Signature { signature } = &self.key {
            if signature.trim().is_empty() {
                return Err(MetricIdentityErrorV1::BlankStableEventField("signature"));
            }
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for StableEventIdentityV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawStableEventIdentityV1::deserialize(deserializer)?;
        let identity = Self {
            source: raw.source,
            key: raw.key,
        };
        identity.validate().map_err(serde::de::Error::custom)?;
        Ok(identity)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricIdentityAvailabilityV1 {
    Available,
    UnavailableLegacySchema,
    UnavailableSourceIdentity,
    ProvenDisjointByPartitionContract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricCollisionClassificationV1 {
    DuplicateRecordIdentity,
    CrossRunJoinKeyDiagnosticOnly,
    StableUnderlyingEventCollision,
    StableUnderlyingEventIdentityUnavailable,
    NoCollision,
}
