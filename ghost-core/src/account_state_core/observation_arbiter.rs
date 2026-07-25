//! Deterministic arbitration for raw Yellowstone account observations.
//!
//! The arbiter is intentionally narrower than the future Observation Ledger:
//! it keeps only the version/hash watermarks and conflict evidence necessary to
//! decide whether an observation may mutate [`AccountStateCore`].  It does not
//! correlate NLN, synthesize a recovery path, or become a second state
//! authority.

use super::types::{
    AccountStateUpdate, AccountUpdateRejectReason, AccountUpdateResult, UpdateSource,
};
use crate::RawProviderRoleV1;
use serde::{Deserialize, Serialize};
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use std::collections::{BTreeMap, HashSet};
use std::str::FromStr;
use std::sync::OnceLock;

/// Chain-version key of one observed account mutation.
///
/// The payload hash intentionally does not belong here.  Keeping it separate
/// makes `same version + different hash` a detectable conflict rather than a
/// second mutation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AccountMutationVersionV1 {
    pub pubkey: Pubkey,
    pub slot: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_version: Option<u64>,
}

/// Stable identity of account contents at a chain mutation version.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountObservationIdentityV1 {
    pub mutation_version: AccountMutationVersionV1,
    /// BLAKE3 of the captured account payload represented by the raw adapter.
    /// The adapter representation is defined by the ingest contract; this is
    /// not a re-hash of decoded reserve fields.
    pub data_hash_blake3: [u8; 32],
}

/// Stable identity of one provider observation of an account payload.
///
/// [`AccountObservationIdentityV1`] deliberately collapses matching payload
/// claims across providers so the arbiter can recognise primary/secondary
/// agreement.  This companion identity retains the provider-specific facts
/// needed to distinguish an exact replay from a same-version/same-hash
/// observation delivered by another provider or transaction context.
///
/// Local receive metadata is intentionally absent: neither timestamp nor
/// receive sequence is a chain identity field.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountProviderObservationIdentityV1 {
    pub account_observation: AccountObservationIdentityV1,
    pub provider_id: String,
    pub provider_role: RawProviderRoleV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub txn_signature: Option<Signature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_or_program: Option<Pubkey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_data_len: Option<u64>,
}

/// One evidence record retained only when an account-version conflict must be
/// explained.  `receive_seq` and `receive_ts_ms` are audit metadata only and
/// never participate in canonical ordering or mutation identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountObservationEvidenceV1 {
    pub identity: AccountObservationIdentityV1,
    pub provider_id: String,
    pub provider_role: RawProviderRoleV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub txn_signature: Option<Signature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_or_program: Option<Pubkey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_data_len: Option<u64>,
    pub receive_ts_ms: u64,
    pub receive_seq: u64,
}

impl AccountObservationEvidenceV1 {
    /// Return the stable provider-observation identity without transport-only
    /// receive metadata.
    #[must_use]
    pub fn provider_observation_identity(&self) -> AccountProviderObservationIdentityV1 {
        AccountProviderObservationIdentityV1 {
            account_observation: self.identity.clone(),
            provider_id: self.provider_id.clone(),
            provider_role: self.provider_role,
            txn_signature: self.txn_signature,
            owner_or_program: self.owner_or_program,
            account_data_len: self.account_data_len,
        }
    }
}

/// All retained evidence for one incompatible payload claim at one exact
/// account mutation version. Unique observations are retained until the
/// explicit per-version capacity is reached; exact retransmissions are not
/// stored again. Capacity exhaustion is a typed, fail-closed outcome, so PR1C
/// never silently discards a conflict witness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountProviderConflictEvidenceV1 {
    pub mutation_version: AccountMutationVersionV1,
    pub observations: Vec<AccountObservationEvidenceV1>,
}

/// Evidence that a single pool/mint mapping attempted to bind two different
/// account pubkeys.  This is separate from a same-version payload conflict.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountIdentityConflictEvidenceV1 {
    pub expected_pubkey: Pubkey,
    pub observed: AccountObservationEvidenceV1,
}

/// Account family derived from the owner program carried by the raw Geyser
/// account observation. `Unknown` is deliberately not interchangeable with a
/// Pump.fun or PumpSwap account and can never authorize an identity change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountSourceAccountKindV1 {
    PumpFunBondingCurve,
    PumpSwapPool,
    Unknown,
}

/// Durable-in-process evidence for the only legal source-account transition
/// handled by PR1C. The transition remains an account-ingest integrity fact;
/// it is not an Observation Ledger record and does not alter policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountIdentityTransitionEvidenceV1 {
    pub previous_pubkey: Pubkey,
    pub previous_source_kind: AccountSourceAccountKindV1,
    pub completion_mutation_version: AccountMutationVersionV1,
    pub next_pubkey: Pubkey,
    pub next_source_kind: AccountSourceAccountKindV1,
    pub transition_mutation_version: AccountMutationVersionV1,
}

/// Bounded-retention scope that prevented the arbiter from retaining another
/// unique piece of evidence. Every such condition is typed and fail-closed;
/// no conflict witness is silently evicted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountObservationEvidenceOverflowScopeV1 {
    VersionIndex,
    VersionObservations,
    /// The bounded provider-conflict evidence store is full. This store is
    /// intentionally separate from the primary ordering watermark lane, so
    /// evidence retention pressure cannot veto a newer eligible primary.
    ProviderConflictEvidence,
    IdentityConflicts,
    IdentityTransitions,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountObservationEvidenceOverflowV1 {
    pub scope: AccountObservationEvidenceOverflowScopeV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation_version: Option<AccountMutationVersionV1>,
    /// Immutable provenance of the first observation that could not be
    /// retained.  Older snapshots did not include this field, hence the
    /// additive `Option`; every overflow produced by PR1C stores `Some`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_rejected_observation: Option<AccountObservationEvidenceV1>,
    pub retained_count: usize,
    pub overflow_count: u64,
}

/// Memory bounds for a single in-process account arbiter.
///
/// These are integrity/resource limits, not Gatekeeper or strategy
/// thresholds. When a bound cannot be respected without losing evidence, the
/// incoming observation is rejected with a typed fail-closed outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountObservationArbiterLimitsV1 {
    /// Bound applied independently to primary-capable records, witness-only
    /// records and retained provider-conflict evidence. The three stores are
    /// deliberately independent: witness or conflict retention pressure must
    /// never consume canonical primary authority capacity.
    pub max_versions_per_account: usize,
    /// Bound applied independently to unique primary and secondary evidence
    /// inside one version record.
    pub max_unique_observations_per_version: usize,
    pub max_identity_conflicts_per_account: usize,
    pub max_identity_transitions_per_account: usize,
}

impl Default for AccountObservationArbiterLimitsV1 {
    fn default() -> Self {
        Self {
            max_versions_per_account: 64,
            max_unique_observations_per_version: 32,
            max_identity_conflicts_per_account: 16,
            max_identity_transitions_per_account: 4,
        }
    }
}

impl AccountObservationArbiterLimitsV1 {
    fn normalized(mut self) -> Self {
        self.max_versions_per_account = self.max_versions_per_account.max(1);
        self.max_unique_observations_per_version = self.max_unique_observations_per_version.max(1);
        self.max_identity_conflicts_per_account = self.max_identity_conflicts_per_account.max(1);
        self.max_identity_transitions_per_account =
            self.max_identity_transitions_per_account.max(1);
        self
    }
}

/// Stable classification assigned exactly once to each observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountObservationClassificationV1 {
    ExactDuplicate,
    NewerMutation,
    OlderObservation,
    SameVersionSameHash,
    SameVersionDifferentHashConflict,
    WriteVersionUnknown,
    MissingProviderProvenance,
    InvalidDataHash,
    UnsupportedUpdateSource,
    AccountIdentityConflict,
    EvidenceCapacityExceeded,
    ArbiterStateUnavailable,
}

impl AccountObservationClassificationV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExactDuplicate => "exact_duplicate",
            Self::NewerMutation => "newer_mutation",
            Self::OlderObservation => "older_observation",
            Self::SameVersionSameHash => "same_version_same_hash",
            Self::SameVersionDifferentHashConflict => "same_version_different_hash_conflict",
            Self::WriteVersionUnknown => "write_version_unknown",
            Self::MissingProviderProvenance => "missing_provider_provenance",
            Self::InvalidDataHash => "invalid_data_hash",
            Self::UnsupportedUpdateSource => "unsupported_update_source",
            Self::AccountIdentityConflict => "account_identity_conflict",
            Self::EvidenceCapacityExceeded => "evidence_capacity_exceeded",
            Self::ArbiterStateUnavailable => "arbiter_state_unavailable",
        }
    }
}

/// Reducer-facing outcome for one observation classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountObservationOutcomeV1 {
    AppliedNewMutation,
    DuplicateObservation,
    StaleObservation,
    ProviderConflict,
    UnorderableWithoutWriteVersion,
    SecondaryWitnessRecorded,
    EvidenceCapacityExceeded,
    RejectedInvalidObservation,
}

impl AccountObservationOutcomeV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AppliedNewMutation => "applied_new_mutation",
            Self::DuplicateObservation => "duplicate_observation",
            Self::StaleObservation => "stale_observation",
            Self::ProviderConflict => "provider_conflict",
            Self::UnorderableWithoutWriteVersion => "unorderable_without_write_version",
            Self::SecondaryWitnessRecorded => "secondary_witness_recorded",
            Self::EvidenceCapacityExceeded => "evidence_capacity_exceeded",
            Self::RejectedInvalidObservation => "rejected_invalid_observation",
        }
    }
}

/// Agreement state between the configured primary and a secondary witness.
/// This is observation evidence only; it does not choose a competing
/// authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountProviderAgreementV1 {
    NotObserved,
    SecondaryWitnessOnly,
    PrimarySecondaryAgreement,
    PrimarySecondaryConflict,
}

impl AccountProviderAgreementV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotObserved => "not_observed",
            Self::SecondaryWitnessOnly => "secondary_witness_only",
            Self::PrimarySecondaryAgreement => "primary_secondary_agreement",
            Self::PrimarySecondaryConflict => "primary_secondary_conflict",
        }
    }
}

/// Complete, typed decision of [`AccountObservationArbiter`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountObservationDecisionV1 {
    pub classification: AccountObservationClassificationV1,
    pub outcome: AccountObservationOutcomeV1,
    pub canonical_apply: bool,
    pub provider_agreement: AccountProviderAgreementV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation_version: Option<AccountMutationVersionV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_hash_blake3: Option<[u8; 32]>,
}

impl AccountObservationDecisionV1 {
    #[must_use]
    pub const fn reject_reason(&self) -> AccountUpdateRejectReason {
        match self.classification {
            AccountObservationClassificationV1::ExactDuplicate
            | AccountObservationClassificationV1::SameVersionSameHash => {
                AccountUpdateRejectReason::DuplicateObservation
            }
            AccountObservationClassificationV1::OlderObservation => {
                AccountUpdateRejectReason::StaleObservation
            }
            AccountObservationClassificationV1::SameVersionDifferentHashConflict
            | AccountObservationClassificationV1::AccountIdentityConflict => {
                AccountUpdateRejectReason::ProviderConflict
            }
            AccountObservationClassificationV1::EvidenceCapacityExceeded => {
                AccountUpdateRejectReason::AccountObservationEvidenceCapacityExceeded
            }
            AccountObservationClassificationV1::WriteVersionUnknown => {
                AccountUpdateRejectReason::UnorderableWithoutWriteVersion
            }
            AccountObservationClassificationV1::MissingProviderProvenance => {
                AccountUpdateRejectReason::MissingProviderProvenance
            }
            AccountObservationClassificationV1::InvalidDataHash => {
                AccountUpdateRejectReason::InvalidAccountDataHash
            }
            AccountObservationClassificationV1::UnsupportedUpdateSource => {
                AccountUpdateRejectReason::UnsupportedAccountUpdateSource
            }
            AccountObservationClassificationV1::ArbiterStateUnavailable => {
                AccountUpdateRejectReason::ArbiterStateUnavailable
            }
            // `NewerMutation` with `canonical_apply = false` can occur only
            // for a correctly recorded secondary witness.
            AccountObservationClassificationV1::NewerMutation => {
                AccountUpdateRejectReason::SecondaryWitness
            }
        }
    }
}

/// Per-account counters owned by the arbiter.  These counters deliberately
/// exclude `CanonicalPoolState` fields, so duplicate/conflict traffic cannot
/// inflate reserves, velocity, state-facing observations, or feature evidence.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountObservationArbiterCountersV1 {
    pub provider_observation_count: u64,
    pub canonical_mutation_count: u64,
    pub duplicate_observation_count: u64,
    pub stale_observation_count: u64,
    pub provider_conflict_count: u64,
    pub unorderable_observation_count: u64,
    pub secondary_witness_count: u64,
    #[serde(default)]
    pub evidence_capacity_exceeded_count: u64,
    #[serde(default)]
    /// Historical serialized name retained for compatibility. It now counts
    /// every pruned, non-latest applied primary watermark record; conflicts
    /// are preserved in the separate bounded evidence store.
    pub pruned_non_conflict_version_count: u64,
    pub invalid_observation_count: u64,
}

/// Read-only diagnostic snapshot of one account arbiter.  This is not the
/// future durable Observation Ledger and is intentionally not consumed by
/// strategy, Gatekeeper, MFS, quote math, or execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountObservationArbiterSnapshotV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_account_pubkey: Option<Pubkey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_primary_canonical: Option<AccountObservationIdentityV1>,
    #[serde(default)]
    pub counters: AccountObservationArbiterCountersV1,
    #[serde(default)]
    pub conflicts: Vec<AccountProviderConflictEvidenceV1>,
    #[serde(default)]
    pub identity_conflicts: Vec<AccountIdentityConflictEvidenceV1>,
    #[serde(default)]
    pub identity_transitions: Vec<AccountIdentityTransitionEvidenceV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_evidence_overflow: Option<AccountObservationEvidenceOverflowV1>,
    /// `false` means the bounded secondary-witness lane overflowed.  A later
    /// eligible primary may still mutate canonical state; only witness
    /// completeness is degraded.
    #[serde(default = "default_true")]
    pub secondary_evidence_complete: bool,
}

const fn default_true() -> bool {
    true
}

impl Default for AccountObservationArbiterSnapshotV1 {
    fn default() -> Self {
        Self {
            bound_account_pubkey: None,
            latest_primary_canonical: None,
            counters: AccountObservationArbiterCountersV1::default(),
            conflicts: Vec::new(),
            identity_conflicts: Vec::new(),
            identity_transitions: Vec::new(),
            first_evidence_overflow: None,
            secondary_evidence_complete: true,
        }
    }
}

/// Full result of applying an account observation through the reducer.
///
/// `canonical_result` is populated only when `decision.canonical_apply` is
/// true.  Compatibility callers may collapse this to `AccountUpdateResult`,
/// but the active Oracle path can retain the typed decision for diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountObservationApplyResultV1 {
    pub decision: AccountObservationDecisionV1,
    pub canonical_result: Option<AccountUpdateResult>,
}

impl AccountObservationApplyResultV1 {
    #[must_use]
    pub const fn did_apply(&self) -> bool {
        self.canonical_result.is_some()
    }

    #[must_use]
    pub fn into_account_update_result(self) -> AccountUpdateResult {
        self.canonical_result
            .unwrap_or_else(|| AccountUpdateResult::Rejected(self.decision.reject_reason()))
    }
}

#[derive(Clone, Debug)]
struct AccountObservationV1 {
    evidence: AccountObservationEvidenceV1,
}

impl AccountObservationV1 {
    fn from_update(update: &AccountStateUpdate) -> Result<Self, InputValidationFailure> {
        if !matches!(
            update.source,
            UpdateSource::GeyserAccountUpdate | UpdateSource::WalReplay
        ) {
            return Err(InputValidationFailure::UnsupportedUpdateSource);
        }

        let Some(provider_role) = update.provider_role else {
            return Err(InputValidationFailure::MissingProviderProvenance);
        };
        let Some(provider_id) = update
            .provider_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
        else {
            return Err(InputValidationFailure::MissingProviderProvenance);
        };
        let Some(data_hash_blake3) = update
            .account_data_hash
            .as_deref()
            .and_then(parse_blake3_hex)
        else {
            return Err(InputValidationFailure::InvalidDataHash);
        };

        let version = AccountMutationVersionV1 {
            // `bonding_curve` is the established account identity at this
            // boundary and is a deterministic compatibility fallback only when
            // an old serialized event omitted `source_account_pubkey`.
            pubkey: update.source_account_pubkey.unwrap_or(update.bonding_curve),
            slot: update.slot,
            write_version: update.write_version,
        };
        Ok(Self {
            evidence: AccountObservationEvidenceV1 {
                identity: AccountObservationIdentityV1 {
                    mutation_version: version,
                    data_hash_blake3,
                },
                provider_id: provider_id.to_owned(),
                provider_role,
                txn_signature: update.txn_signature,
                owner_or_program: update.source_account_owner_or_program,
                account_data_len: update.account_data_len,
                receive_ts_ms: update.receive_ts_ms,
                receive_seq: update.receive_seq,
            },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputValidationFailure {
    MissingProviderProvenance,
    InvalidDataHash,
    UnsupportedUpdateSource,
}

#[derive(Clone, Debug)]
struct VersionEvidence {
    first: AccountObservationEvidenceV1,
    seen_payload_hashes: HashSet<[u8; 32]>,
    exact_observation_keys: HashSet<AccountProviderObservationIdentityV1>,
    /// The retained, unique evidence for this version. Exact retransmissions
    /// are counted by the arbiter but are not stored again; retaining them
    /// would make a reconnect burst unbounded without adding conflict facts.
    observations: Vec<AccountObservationEvidenceV1>,
    /// Primary and secondary observations have independently bounded lanes.
    /// This reservation is what prevents secondary evidence saturation from
    /// becoming an authority veto over the first eligible primary.
    primary_observation_count: usize,
    secondary_observation_count: usize,
    has_primary: bool,
    has_secondary: bool,
    primary_applied_hash: Option<[u8; 32]>,
    /// This is only the local fact that incompatible payload hashes were seen.
    /// The auditable payloads themselves live in `provider_conflicts`, outside
    /// the bounded primary ordering lane, so a conflict cannot poison
    /// primary-watermark pruneability.
    has_payload_conflict: bool,
}

impl VersionEvidence {
    fn new(observation: AccountObservationEvidenceV1) -> Self {
        let mut exact_observation_keys = HashSet::new();
        exact_observation_keys.insert(observation.provider_observation_identity());
        let has_primary = matches!(
            observation.provider_role,
            RawProviderRoleV1::PrimaryAuthority
        );
        let has_secondary = matches!(
            observation.provider_role,
            RawProviderRoleV1::SecondaryWitness
        );
        Self {
            first: observation.clone(),
            seen_payload_hashes: HashSet::from([observation.identity.data_hash_blake3]),
            exact_observation_keys,
            observations: vec![observation],
            primary_observation_count: usize::from(has_primary),
            secondary_observation_count: usize::from(has_secondary),
            has_primary,
            has_secondary,
            primary_applied_hash: None,
            has_payload_conflict: false,
        }
    }

    fn try_record(
        &mut self,
        observation: &AccountObservationEvidenceV1,
        max_unique_observations: usize,
    ) -> Result<RecordedObservation, ()> {
        let key = observation.provider_observation_identity();
        if self.exact_observation_keys.contains(&key) {
            self.note_provider_role(observation.provider_role);
            return Ok(RecordedObservation {
                exact_duplicate: true,
                introduced_payload_conflict: false,
            });
        }
        let lane_count = match observation.provider_role {
            RawProviderRoleV1::PrimaryAuthority => self.primary_observation_count,
            RawProviderRoleV1::SecondaryWitness => self.secondary_observation_count,
        };
        if lane_count >= max_unique_observations {
            return Err(());
        }

        self.note_provider_role(observation.provider_role);
        match observation.provider_role {
            RawProviderRoleV1::PrimaryAuthority => {
                self.primary_observation_count = self.primary_observation_count.saturating_add(1);
            }
            RawProviderRoleV1::SecondaryWitness => {
                self.secondary_observation_count =
                    self.secondary_observation_count.saturating_add(1);
            }
        }
        self.exact_observation_keys.insert(key);
        self.observations.push(observation.clone());
        let introduced_payload_conflict = self
            .seen_payload_hashes
            .insert(observation.identity.data_hash_blake3);
        if self.seen_payload_hashes.len() > 1 {
            self.has_payload_conflict = true;
        }
        Ok(RecordedObservation {
            exact_duplicate: false,
            introduced_payload_conflict,
        })
    }

    fn note_provider_role(&mut self, provider_role: RawProviderRoleV1) {
        match provider_role {
            RawProviderRoleV1::PrimaryAuthority => self.has_primary = true,
            RawProviderRoleV1::SecondaryWitness => self.has_secondary = true,
        }
    }

    #[must_use]
    const fn provider_agreement(&self) -> AccountProviderAgreementV1 {
        if self.has_payload_conflict && self.has_primary && self.has_secondary {
            AccountProviderAgreementV1::PrimarySecondaryConflict
        } else if self.has_primary && self.has_secondary {
            AccountProviderAgreementV1::PrimarySecondaryAgreement
        } else if self.has_secondary {
            AccountProviderAgreementV1::SecondaryWitnessOnly
        } else {
            AccountProviderAgreementV1::NotObserved
        }
    }

    #[must_use]
    fn can_be_pruned(&self, latest: Option<&AccountObservationIdentityV1>) -> bool {
        self.primary_applied_hash.is_some()
            && latest.is_some_and(|latest| {
                latest.mutation_version != self.first.identity.mutation_version
            })
    }

    #[must_use]
    const fn retained_observation_count(&self) -> usize {
        self.observations.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RecordedObservation {
    exact_duplicate: bool,
    introduced_payload_conflict: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VersionRelation {
    Newer,
    Older,
    Same,
    WriteVersionUnknown,
}

#[derive(Clone, Copy, Debug)]
struct BoundAccountIdentity {
    pubkey: Pubkey,
    source_kind: AccountSourceAccountKindV1,
}

#[derive(Clone, Debug)]
struct PrimaryPumpFunCompletion {
    account_pubkey: Pubkey,
    mutation_version: AccountMutationVersionV1,
}

#[derive(Clone, Debug)]
enum IdentityDisposition {
    NoBinding,
    Initial(BoundAccountIdentity),
    Transition(AccountIdentityTransitionEvidenceV1),
    Conflict { expected_pubkey: Pubkey },
    EvidenceOverflow(AccountObservationEvidenceOverflowScopeV1),
}

/// Typed, per-account arbiter. It is synchronously serialized by
/// `AccountStateReducer` for the matching base mint; it never crosses an
/// async await point or uses `receive_seq` as a chain-ordering field.
///
/// Exactly-once means *within this process lifetime*. PR1C has no durable
/// Observation Ledger or startup hydration path; cross-restart reconciliation
/// is deliberately deferred to PR1D rather than being implied by this type.
#[derive(Debug)]
pub struct AccountObservationArbiter {
    limits: AccountObservationArbiterLimitsV1,
    bound_account_identity: Option<BoundAccountIdentity>,
    latest_primary_canonical: Option<AccountObservationIdentityV1>,
    primary_pumpfun_completion: Option<PrimaryPumpFunCompletion>,
    /// Primary-capable records. Only a primary observation may create or
    /// promote a record into this map, so secondary traffic cannot exhaust the
    /// canonical authority lane.
    primary_versions: BTreeMap<AccountMutationVersionV1, VersionEvidence>,
    /// Bounded witness-only records retained until the matching primary
    /// arrives. They are intentionally separate from `primary_versions`:
    /// witness saturation must degrade evidence completeness, never veto a
    /// later eligible primary mutation.
    secondary_witness_versions: BTreeMap<AccountMutationVersionV1, VersionEvidence>,
    /// Bounded, append-only-within-process provider-conflict evidence. It is
    /// intentionally separate from `primary_versions`: old primary ordering
    /// watermarks may rotate even when a secondary has supplied a conflicting
    /// payload for that version.
    provider_conflicts: BTreeMap<AccountMutationVersionV1, AccountProviderConflictEvidenceV1>,
    secondary_evidence_complete: bool,
    counters: AccountObservationArbiterCountersV1,
    identity_conflicts: Vec<AccountIdentityConflictEvidenceV1>,
    identity_transitions: Vec<AccountIdentityTransitionEvidenceV1>,
    first_evidence_overflow: Option<AccountObservationEvidenceOverflowV1>,
}

impl Default for AccountObservationArbiter {
    fn default() -> Self {
        Self::with_limits(AccountObservationArbiterLimitsV1::default())
    }
}

impl AccountObservationArbiter {
    #[must_use]
    pub fn with_limits(limits: AccountObservationArbiterLimitsV1) -> Self {
        Self {
            limits: limits.normalized(),
            bound_account_identity: None,
            latest_primary_canonical: None,
            primary_pumpfun_completion: None,
            primary_versions: BTreeMap::new(),
            secondary_witness_versions: BTreeMap::new(),
            provider_conflicts: BTreeMap::new(),
            secondary_evidence_complete: true,
            counters: AccountObservationArbiterCountersV1::default(),
            identity_conflicts: Vec::new(),
            identity_transitions: Vec::new(),
            first_evidence_overflow: None,
        }
    }

    #[must_use]
    pub fn arbitrate(&mut self, update: &AccountStateUpdate) -> AccountObservationDecisionV1 {
        self.counters.provider_observation_count =
            self.counters.provider_observation_count.saturating_add(1);

        let fallback_version = AccountMutationVersionV1 {
            pubkey: update.source_account_pubkey.unwrap_or(update.bonding_curve),
            slot: update.slot,
            write_version: update.write_version,
        };
        let observation = match AccountObservationV1::from_update(update) {
            Ok(observation) => observation,
            Err(failure) => return self.invalid_decision(failure, fallback_version),
        };
        let version = observation.evidence.identity.mutation_version.clone();
        let payload_hash = observation.evidence.identity.data_hash_blake3;
        let is_secondary = matches!(
            observation.evidence.provider_role,
            RawProviderRoleV1::SecondaryWitness
        );
        let latest = self.latest_primary_canonical.clone();
        let identity_disposition =
            self.identity_disposition(&observation.evidence, latest.as_ref());
        match identity_disposition {
            IdentityDisposition::Conflict { expected_pubkey } => {
                if self.identity_conflicts.len() >= self.limits.max_identity_conflicts_per_account {
                    return self.evidence_capacity_decision(
                        AccountObservationEvidenceOverflowScopeV1::IdentityConflicts,
                        &observation.evidence,
                        self.identity_conflicts.len(),
                    );
                }
                self.identity_conflicts
                    .push(AccountIdentityConflictEvidenceV1 {
                        expected_pubkey,
                        observed: observation.evidence.clone(),
                    });
                return self.finish(
                    AccountObservationClassificationV1::AccountIdentityConflict,
                    AccountObservationOutcomeV1::ProviderConflict,
                    false,
                    AccountProviderAgreementV1::NotObserved,
                    Some(version),
                    Some(payload_hash),
                );
            }
            IdentityDisposition::EvidenceOverflow(scope) => {
                return self.evidence_capacity_decision(
                    scope,
                    &observation.evidence,
                    self.identity_transitions.len(),
                );
            }
            IdentityDisposition::NoBinding
            | IdentityDisposition::Initial(_)
            | IdentityDisposition::Transition(_) => {}
        }

        if self.primary_versions.contains_key(&version) {
            return self.arbitrate_primary_version(
                version,
                observation.evidence,
                update,
                latest.as_ref(),
                &identity_disposition,
            );
        }

        if is_secondary {
            return self.arbitrate_secondary_witness_version(
                version,
                observation.evidence,
                latest.as_ref(),
            );
        }

        // The primary ordering lane is a canonical watermark, not a cache of
        // every primary delivery.  A known-stale or unorderable primary has no
        // possible canonical effect, so retaining it here would let replay
        // traffic exhaust capacity needed by a later newer primary mutation.
        // Any matching secondary witness remains in its separate bounded lane.
        let relation = relation_to_latest(&version, latest.as_ref());
        if matches!(
            relation,
            VersionRelation::Older | VersionRelation::WriteVersionUnknown
        ) {
            // A stale/unorderable primary cannot move canonical state, but it
            // may still complete or contradict an already retained secondary
            // witness for the exact same version.  Correlate it in the
            // witness lane without promoting it into `primary_versions`.
            if self.secondary_witness_versions.contains_key(&version) {
                return self.arbitrate_noncanonical_primary_witness_version(
                    version,
                    observation.evidence,
                    relation,
                );
            }
            return self.record_decision(decision_for_primary_relation(
                relation,
                AccountProviderAgreementV1::NotObserved,
                version,
                payload_hash,
            ));
        }

        // Promote retained secondary evidence into the primary-capable lane
        // only once its version is eligible for canonical ordering. The
        // dedicated primary lane is never consumed by witness-only or stale
        // primary traffic.
        if let Some(record) = self.secondary_witness_versions.remove(&version) {
            if !self.reserve_primary_version_slot() {
                self.secondary_witness_versions
                    .insert(version.clone(), record);
                return self.evidence_capacity_decision(
                    AccountObservationEvidenceOverflowScopeV1::VersionIndex,
                    &observation.evidence,
                    self.primary_versions.len(),
                );
            }
            self.primary_versions.insert(version.clone(), record);
            return self.arbitrate_primary_version(
                version,
                observation.evidence,
                update,
                latest.as_ref(),
                &identity_disposition,
            );
        }

        if !self.reserve_primary_version_slot() {
            return self.evidence_capacity_decision(
                AccountObservationEvidenceOverflowScopeV1::VersionIndex,
                &observation.evidence,
                self.primary_versions.len(),
            );
        }

        let mut record = VersionEvidence::new(observation.evidence.clone());
        let agreement = record.provider_agreement();
        let decision =
            decision_for_primary_relation(relation, agreement, version.clone(), payload_hash);
        if decision.canonical_apply {
            record.primary_applied_hash = Some(payload_hash);
        }
        self.primary_versions.insert(version.clone(), record);
        if decision.canonical_apply {
            self.mark_primary_apply(
                &version,
                payload_hash,
                &observation.evidence,
                update,
                &identity_disposition,
            );
        }
        self.record_decision(decision)
    }

    fn arbitrate_primary_version(
        &mut self,
        version: AccountMutationVersionV1,
        evidence: AccountObservationEvidenceV1,
        update: &AccountStateUpdate,
        latest: Option<&AccountObservationIdentityV1>,
        identity_disposition: &IdentityDisposition,
    ) -> AccountObservationDecisionV1 {
        let payload_hash = evidence.identity.data_hash_blake3;
        let recorded = {
            let record = self
                .primary_versions
                .get_mut(&version)
                .expect("primary version was inserted before arbitration");
            record.try_record(&evidence, self.limits.max_unique_observations_per_version)
        };
        let recorded = match recorded {
            Ok(recorded) => recorded,
            Err(()) => {
                let retained_count = self
                    .primary_versions
                    .get(&version)
                    .map_or(0, VersionEvidence::retained_observation_count);
                return self.evidence_capacity_decision(
                    AccountObservationEvidenceOverflowScopeV1::VersionObservations,
                    &evidence,
                    retained_count,
                );
            }
        };

        let (agreement, primary_applied_hash, conflict_present, has_secondary, observations) = self
            .primary_versions
            .get(&version)
            .map(|record| {
                (
                    record.provider_agreement(),
                    record.primary_applied_hash,
                    record.has_payload_conflict,
                    record.has_secondary,
                    record.observations.clone(),
                )
            })
            .expect("primary record remains present after bounded observation recording");

        if conflict_present && !self.retain_provider_conflict_evidence(&version, &observations) {
            // A secondary conflict-store overflow is already visible as typed
            // degraded evidence. It must not prevent the first eligible
            // primary payload for this version from becoming canonical.
            let primary_can_apply =
                matches!(relation_to_latest(&version, latest), VersionRelation::Newer)
                    && primary_applied_hash.is_none()
                    && matches!(evidence.provider_role, RawProviderRoleV1::PrimaryAuthority);
            if !primary_can_apply {
                return self.evidence_capacity_decision(
                    AccountObservationEvidenceOverflowScopeV1::ProviderConflictEvidence,
                    &evidence,
                    self.provider_conflicts.len(),
                );
            }
            // The primary mutation must still apply, but the missing retained
            // conflict evidence is observable.  In this branch a secondary
            // has already supplied a competing payload, so the witness-side
            // completeness bit must degrade even though the rejected snapshot
            // is the eligible primary observation that exposed saturation.
            self.note_evidence_overflow(
                AccountObservationEvidenceOverflowScopeV1::ProviderConflictEvidence,
                &evidence,
                self.provider_conflicts.len(),
                has_secondary,
            );
        }

        if matches!(evidence.provider_role, RawProviderRoleV1::SecondaryWitness) {
            if recorded.introduced_payload_conflict {
                return self.finish(
                    AccountObservationClassificationV1::SameVersionDifferentHashConflict,
                    AccountObservationOutcomeV1::ProviderConflict,
                    false,
                    agreement,
                    Some(version),
                    Some(payload_hash),
                );
            }
            return self.finish(
                if recorded.exact_duplicate {
                    AccountObservationClassificationV1::ExactDuplicate
                } else {
                    AccountObservationClassificationV1::SameVersionSameHash
                },
                AccountObservationOutcomeV1::SecondaryWitnessRecorded,
                false,
                agreement,
                Some(version),
                Some(payload_hash),
            );
        }

        if let Some(primary_hash) = primary_applied_hash {
            if primary_hash == payload_hash {
                return self.finish(
                    if recorded.exact_duplicate {
                        AccountObservationClassificationV1::ExactDuplicate
                    } else {
                        AccountObservationClassificationV1::SameVersionSameHash
                    },
                    AccountObservationOutcomeV1::DuplicateObservation,
                    false,
                    agreement,
                    Some(version),
                    Some(payload_hash),
                );
            }
            return self.finish(
                AccountObservationClassificationV1::SameVersionDifferentHashConflict,
                AccountObservationOutcomeV1::ProviderConflict,
                false,
                agreement,
                Some(version),
                Some(payload_hash),
            );
        }

        // Secondary evidence may be promoted into this record, but it can
        // never veto the first eligible primary mutation.
        let relation = relation_to_latest(&version, latest);
        let decision =
            decision_for_primary_relation(relation, agreement, version.clone(), payload_hash);
        if decision.canonical_apply {
            self.mark_primary_apply(
                &version,
                payload_hash,
                &evidence,
                update,
                identity_disposition,
            );
        } else if conflict_present && !recorded.exact_duplicate {
            return self.finish(
                AccountObservationClassificationV1::SameVersionDifferentHashConflict,
                AccountObservationOutcomeV1::ProviderConflict,
                false,
                agreement,
                Some(version),
                Some(payload_hash),
            );
        }
        self.record_decision(decision)
    }

    /// Correlate a primary that is known not to be canonical-order eligible
    /// with previously retained witness evidence for its exact version.  This
    /// preserves agreement/conflict facts without consuming primary ordering
    /// capacity or changing canonical state.
    fn arbitrate_noncanonical_primary_witness_version(
        &mut self,
        version: AccountMutationVersionV1,
        evidence: AccountObservationEvidenceV1,
        relation: VersionRelation,
    ) -> AccountObservationDecisionV1 {
        debug_assert!(matches!(
            relation,
            VersionRelation::Older | VersionRelation::WriteVersionUnknown
        ));
        let payload_hash = evidence.identity.data_hash_blake3;
        let recorded = {
            let record = self.secondary_witness_versions.get_mut(&version).expect(
                "noncanonical primary correlation requires a retained secondary witness record",
            );
            record.try_record(&evidence, self.limits.max_unique_observations_per_version)
        };
        let recorded = match recorded {
            Ok(recorded) => recorded,
            Err(()) => {
                let retained_count = self
                    .secondary_witness_versions
                    .get(&version)
                    .map_or(0, VersionEvidence::retained_observation_count);
                return self.evidence_capacity_decision(
                    AccountObservationEvidenceOverflowScopeV1::VersionObservations,
                    &evidence,
                    retained_count,
                );
            }
        };

        let (agreement, conflict_present, has_secondary, observations) = self
            .secondary_witness_versions
            .get(&version)
            .map(|record| {
                (
                    record.provider_agreement(),
                    record.has_payload_conflict,
                    record.has_secondary,
                    record.observations.clone(),
                )
            })
            .expect("secondary witness record remains present after primary correlation");

        if conflict_present && !self.retain_provider_conflict_evidence(&version, &observations) {
            return self.evidence_capacity_decision_with_secondary_loss(
                AccountObservationEvidenceOverflowScopeV1::ProviderConflictEvidence,
                &evidence,
                self.provider_conflicts.len(),
                has_secondary,
            );
        }

        if recorded.introduced_payload_conflict {
            return self.finish(
                AccountObservationClassificationV1::SameVersionDifferentHashConflict,
                AccountObservationOutcomeV1::ProviderConflict,
                false,
                agreement,
                Some(version),
                Some(payload_hash),
            );
        }

        self.finish(
            if recorded.exact_duplicate {
                AccountObservationClassificationV1::ExactDuplicate
            } else {
                AccountObservationClassificationV1::SameVersionSameHash
            },
            AccountObservationOutcomeV1::DuplicateObservation,
            false,
            agreement,
            Some(version),
            Some(payload_hash),
        )
    }

    fn arbitrate_secondary_witness_version(
        &mut self,
        version: AccountMutationVersionV1,
        evidence: AccountObservationEvidenceV1,
        latest: Option<&AccountObservationIdentityV1>,
    ) -> AccountObservationDecisionV1 {
        let payload_hash = evidence.identity.data_hash_blake3;
        if self.secondary_witness_versions.contains_key(&version) {
            let recorded = {
                let record = self
                    .secondary_witness_versions
                    .get_mut(&version)
                    .expect("secondary witness version was checked immediately before get_mut");
                record.try_record(&evidence, self.limits.max_unique_observations_per_version)
            };
            let recorded = match recorded {
                Ok(recorded) => recorded,
                Err(()) => {
                    let retained_count = self
                        .secondary_witness_versions
                        .get(&version)
                        .map_or(0, VersionEvidence::retained_observation_count);
                    return self.evidence_capacity_decision(
                        AccountObservationEvidenceOverflowScopeV1::VersionObservations,
                        &evidence,
                        retained_count,
                    );
                }
            };
            let (agreement, conflict_present, observations) = self
                .secondary_witness_versions
                .get(&version)
                .map(|record| {
                    (
                        record.provider_agreement(),
                        record.has_payload_conflict,
                        record.observations.clone(),
                    )
                })
                .expect("secondary witness record remains present after recording");
            if conflict_present && !self.retain_provider_conflict_evidence(&version, &observations)
            {
                return self.evidence_capacity_decision(
                    AccountObservationEvidenceOverflowScopeV1::ProviderConflictEvidence,
                    &evidence,
                    self.provider_conflicts.len(),
                );
            }
            if recorded.introduced_payload_conflict {
                return self.finish(
                    AccountObservationClassificationV1::SameVersionDifferentHashConflict,
                    AccountObservationOutcomeV1::ProviderConflict,
                    false,
                    agreement,
                    Some(version),
                    Some(payload_hash),
                );
            }
            return self.finish(
                if recorded.exact_duplicate {
                    AccountObservationClassificationV1::ExactDuplicate
                } else {
                    AccountObservationClassificationV1::SameVersionSameHash
                },
                AccountObservationOutcomeV1::SecondaryWitnessRecorded,
                false,
                agreement,
                Some(version),
                Some(payload_hash),
            );
        }

        if !self.reserve_secondary_witness_version_slot() {
            return self.evidence_capacity_decision(
                AccountObservationEvidenceOverflowScopeV1::VersionIndex,
                &evidence,
                self.secondary_witness_versions.len(),
            );
        }

        let relation = relation_to_latest(&version, latest);
        let record = VersionEvidence::new(evidence);
        let agreement = record.provider_agreement();
        let decision = self.decision_for_secondary_relation(
            relation,
            agreement,
            version.clone(),
            payload_hash,
        );
        self.secondary_witness_versions.insert(version, record);
        self.record_decision(decision)
    }

    #[must_use]
    pub fn snapshot(&self) -> AccountObservationArbiterSnapshotV1 {
        AccountObservationArbiterSnapshotV1 {
            bound_account_pubkey: self.bound_account_identity.map(|bound| bound.pubkey),
            latest_primary_canonical: self.latest_primary_canonical.clone(),
            counters: self.counters.clone(),
            conflicts: self.provider_conflicts.values().cloned().collect(),
            identity_conflicts: self.identity_conflicts.clone(),
            identity_transitions: self.identity_transitions.clone(),
            first_evidence_overflow: self.first_evidence_overflow.clone(),
            secondary_evidence_complete: self.secondary_evidence_complete,
        }
    }

    fn identity_disposition(
        &self,
        observation: &AccountObservationEvidenceV1,
        latest: Option<&AccountObservationIdentityV1>,
    ) -> IdentityDisposition {
        let is_primary = matches!(
            observation.provider_role,
            RawProviderRoleV1::PrimaryAuthority
        );
        let incoming_kind = source_account_kind(observation.owner_or_program);
        let incoming_pubkey = observation.identity.mutation_version.pubkey;
        let Some(bound) = self.bound_account_identity else {
            return if is_primary {
                IdentityDisposition::Initial(BoundAccountIdentity {
                    pubkey: incoming_pubkey,
                    source_kind: incoming_kind,
                })
            } else {
                // A witness is not allowed to choose the account identity that
                // a later primary must use.
                IdentityDisposition::NoBinding
            };
        };
        if bound.pubkey == incoming_pubkey {
            return IdentityDisposition::NoBinding;
        }

        let migration_ready = self
            .primary_pumpfun_completion
            .as_ref()
            .is_some_and(|completion| {
                completion.account_pubkey == bound.pubkey
                    && matches!(
                        relation_to_latest(&observation.identity.mutation_version, latest),
                        VersionRelation::Newer
                    )
            });
        if is_primary
            && bound.source_kind == AccountSourceAccountKindV1::PumpFunBondingCurve
            && incoming_kind == AccountSourceAccountKindV1::PumpSwapPool
            && migration_ready
        {
            if self.identity_transitions.len() >= self.limits.max_identity_transitions_per_account {
                return IdentityDisposition::EvidenceOverflow(
                    AccountObservationEvidenceOverflowScopeV1::IdentityTransitions,
                );
            }
            let completion = self
                .primary_pumpfun_completion
                .as_ref()
                .expect("migration_ready requires a completion evidence record");
            return IdentityDisposition::Transition(AccountIdentityTransitionEvidenceV1 {
                previous_pubkey: bound.pubkey,
                previous_source_kind: bound.source_kind,
                completion_mutation_version: completion.mutation_version.clone(),
                next_pubkey: incoming_pubkey,
                next_source_kind: incoming_kind,
                transition_mutation_version: observation.identity.mutation_version.clone(),
            });
        }
        IdentityDisposition::Conflict {
            expected_pubkey: bound.pubkey,
        }
    }

    fn mark_primary_apply(
        &mut self,
        version: &AccountMutationVersionV1,
        payload_hash: [u8; 32],
        evidence: &AccountObservationEvidenceV1,
        update: &AccountStateUpdate,
        identity_disposition: &IdentityDisposition,
    ) {
        if let Some(record) = self.primary_versions.get_mut(version) {
            record.primary_applied_hash = Some(payload_hash);
        }
        self.latest_primary_canonical = Some(AccountObservationIdentityV1 {
            mutation_version: version.clone(),
            data_hash_blake3: payload_hash,
        });
        match identity_disposition {
            IdentityDisposition::Initial(bound) => self.bound_account_identity = Some(*bound),
            IdentityDisposition::Transition(transition) => {
                self.bound_account_identity = Some(BoundAccountIdentity {
                    pubkey: transition.next_pubkey,
                    source_kind: transition.next_source_kind,
                });
                self.identity_transitions.push(transition.clone());
            }
            IdentityDisposition::NoBinding
            | IdentityDisposition::Conflict { .. }
            | IdentityDisposition::EvidenceOverflow(_) => {}
        }
        if update.is_complete != 0
            && source_account_kind(evidence.owner_or_program)
                == AccountSourceAccountKindV1::PumpFunBondingCurve
        {
            self.primary_pumpfun_completion = Some(PrimaryPumpFunCompletion {
                account_pubkey: version.pubkey,
                mutation_version: version.clone(),
            });
        }
    }

    fn reserve_primary_version_slot(&mut self) -> bool {
        if self.primary_versions.len() < self.limits.max_versions_per_account {
            return true;
        }
        let latest = self.latest_primary_canonical.as_ref();
        let evictable = self
            .primary_versions
            .iter()
            .find_map(|(version, record)| record.can_be_pruned(latest).then(|| version.clone()));
        let Some(version) = evictable else {
            return false;
        };
        self.primary_versions.remove(&version);
        // Conflict payloads were retained independently in
        // `provider_conflicts`, so pruning this ordering watermark cannot
        // silently discard a primary/secondary disagreement.
        self.counters.pruned_non_conflict_version_count = self
            .counters
            .pruned_non_conflict_version_count
            .saturating_add(1);
        true
    }

    /// Secondary-only evidence deliberately has its own bounded index.  Once
    /// it is full, later witnesses are represented by typed overflow evidence
    /// rather than evicting retained facts or consuming primary authority
    /// capacity.
    fn reserve_secondary_witness_version_slot(&self) -> bool {
        self.secondary_witness_versions.len() < self.limits.max_versions_per_account
    }

    /// Retains provider-conflict evidence without making it part of canonical
    /// primary ordering capacity. The store has its own bounded version index
    /// and per-role observation limits. Replays do not consume capacity.
    fn retain_provider_conflict_evidence(
        &mut self,
        version: &AccountMutationVersionV1,
        current_version_observations: &[AccountObservationEvidenceV1],
    ) -> bool {
        let max_unique_observations_per_version = self.limits.max_unique_observations_per_version;
        if let Some(existing) = self.provider_conflicts.get_mut(version) {
            for candidate in current_version_observations {
                let candidate_key = candidate.provider_observation_identity();
                if existing
                    .observations
                    .iter()
                    .any(|evidence| evidence.provider_observation_identity() == candidate_key)
                {
                    continue;
                }
                let retained_in_same_role = existing
                    .observations
                    .iter()
                    .filter(|evidence| evidence.provider_role == candidate.provider_role)
                    .count();
                if retained_in_same_role >= max_unique_observations_per_version {
                    return false;
                }
                existing.observations.push(candidate.clone());
            }
            return true;
        }

        if self.provider_conflicts.len() >= self.limits.max_versions_per_account {
            return false;
        }
        self.provider_conflicts.insert(
            version.clone(),
            AccountProviderConflictEvidenceV1 {
                mutation_version: version.clone(),
                observations: current_version_observations.to_vec(),
            },
        );
        true
    }

    fn evidence_capacity_decision(
        &mut self,
        scope: AccountObservationEvidenceOverflowScopeV1,
        rejected_observation: &AccountObservationEvidenceV1,
        retained_count: usize,
    ) -> AccountObservationDecisionV1 {
        self.evidence_capacity_decision_with_secondary_loss(
            scope,
            rejected_observation,
            retained_count,
            matches!(
                rejected_observation.provider_role,
                RawProviderRoleV1::SecondaryWitness
            ),
        )
    }

    fn evidence_capacity_decision_with_secondary_loss(
        &mut self,
        scope: AccountObservationEvidenceOverflowScopeV1,
        rejected_observation: &AccountObservationEvidenceV1,
        retained_count: usize,
        secondary_evidence_lost: bool,
    ) -> AccountObservationDecisionV1 {
        self.note_evidence_overflow(
            scope,
            rejected_observation,
            retained_count,
            secondary_evidence_lost,
        );
        self.finish(
            AccountObservationClassificationV1::EvidenceCapacityExceeded,
            AccountObservationOutcomeV1::EvidenceCapacityExceeded,
            false,
            AccountProviderAgreementV1::NotObserved,
            Some(rejected_observation.identity.mutation_version.clone()),
            Some(rejected_observation.identity.data_hash_blake3),
        )
    }

    /// Record an evidence-retention loss without necessarily changing the
    /// canonical decision.  A conflict-store overflow discovered while an
    /// eligible primary applies is degraded evidence, not a secondary veto.
    fn note_evidence_overflow(
        &mut self,
        scope: AccountObservationEvidenceOverflowScopeV1,
        rejected_observation: &AccountObservationEvidenceV1,
        retained_count: usize,
        secondary_evidence_lost: bool,
    ) {
        self.counters.evidence_capacity_exceeded_count = self
            .counters
            .evidence_capacity_exceeded_count
            .saturating_add(1);
        let overflow_count = self.counters.evidence_capacity_exceeded_count;
        if secondary_evidence_lost {
            self.secondary_evidence_complete = false;
        }
        let mutation_version = Some(rejected_observation.identity.mutation_version.clone());
        self.first_evidence_overflow
            .get_or_insert(AccountObservationEvidenceOverflowV1 {
                scope,
                mutation_version: mutation_version.clone(),
                first_rejected_observation: Some(rejected_observation.clone()),
                retained_count,
                overflow_count,
            });
    }

    fn invalid_decision(
        &mut self,
        failure: InputValidationFailure,
        version: AccountMutationVersionV1,
    ) -> AccountObservationDecisionV1 {
        let classification = match failure {
            InputValidationFailure::MissingProviderProvenance => {
                AccountObservationClassificationV1::MissingProviderProvenance
            }
            InputValidationFailure::InvalidDataHash => {
                AccountObservationClassificationV1::InvalidDataHash
            }
            InputValidationFailure::UnsupportedUpdateSource => {
                AccountObservationClassificationV1::UnsupportedUpdateSource
            }
        };
        self.finish(
            classification,
            AccountObservationOutcomeV1::RejectedInvalidObservation,
            false,
            AccountProviderAgreementV1::NotObserved,
            Some(version),
            None,
        )
    }

    fn decision_for_secondary_relation(
        &self,
        relation: VersionRelation,
        agreement: AccountProviderAgreementV1,
        version: AccountMutationVersionV1,
        data_hash_blake3: [u8; 32],
    ) -> AccountObservationDecisionV1 {
        let classification = match relation {
            VersionRelation::Newer | VersionRelation::Same => {
                AccountObservationClassificationV1::NewerMutation
            }
            VersionRelation::Older => AccountObservationClassificationV1::OlderObservation,
            VersionRelation::WriteVersionUnknown => {
                AccountObservationClassificationV1::WriteVersionUnknown
            }
        };
        AccountObservationDecisionV1 {
            classification,
            outcome: AccountObservationOutcomeV1::SecondaryWitnessRecorded,
            canonical_apply: false,
            provider_agreement: agreement,
            mutation_version: Some(version),
            data_hash_blake3: Some(data_hash_blake3),
        }
    }

    fn finish(
        &mut self,
        classification: AccountObservationClassificationV1,
        outcome: AccountObservationOutcomeV1,
        canonical_apply: bool,
        provider_agreement: AccountProviderAgreementV1,
        mutation_version: Option<AccountMutationVersionV1>,
        data_hash_blake3: Option<[u8; 32]>,
    ) -> AccountObservationDecisionV1 {
        self.record_decision(AccountObservationDecisionV1 {
            classification,
            outcome,
            canonical_apply,
            provider_agreement,
            mutation_version,
            data_hash_blake3,
        })
    }

    fn record_decision(
        &mut self,
        decision: AccountObservationDecisionV1,
    ) -> AccountObservationDecisionV1 {
        match decision.outcome {
            AccountObservationOutcomeV1::AppliedNewMutation => {
                self.counters.canonical_mutation_count =
                    self.counters.canonical_mutation_count.saturating_add(1);
            }
            AccountObservationOutcomeV1::DuplicateObservation => {
                self.counters.duplicate_observation_count =
                    self.counters.duplicate_observation_count.saturating_add(1);
            }
            AccountObservationOutcomeV1::StaleObservation => {
                self.counters.stale_observation_count =
                    self.counters.stale_observation_count.saturating_add(1);
            }
            AccountObservationOutcomeV1::ProviderConflict => {
                self.counters.provider_conflict_count =
                    self.counters.provider_conflict_count.saturating_add(1);
            }
            AccountObservationOutcomeV1::UnorderableWithoutWriteVersion => {
                self.counters.unorderable_observation_count = self
                    .counters
                    .unorderable_observation_count
                    .saturating_add(1);
            }
            AccountObservationOutcomeV1::SecondaryWitnessRecorded => {
                self.counters.secondary_witness_count =
                    self.counters.secondary_witness_count.saturating_add(1);
            }
            AccountObservationOutcomeV1::EvidenceCapacityExceeded => {}
            AccountObservationOutcomeV1::RejectedInvalidObservation => {
                self.counters.invalid_observation_count =
                    self.counters.invalid_observation_count.saturating_add(1);
            }
        }
        decision
    }
}

const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMP_SWAP_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";

fn pump_fun_program_id() -> Pubkey {
    static PROGRAM_ID: OnceLock<Pubkey> = OnceLock::new();
    *PROGRAM_ID.get_or_init(|| {
        Pubkey::from_str(PUMP_FUN_PROGRAM_ID)
            .expect("pump.fun program ID constant must be a valid pubkey")
    })
}

fn pump_swap_program_id() -> Pubkey {
    static PROGRAM_ID: OnceLock<Pubkey> = OnceLock::new();
    *PROGRAM_ID.get_or_init(|| {
        Pubkey::from_str(PUMP_SWAP_PROGRAM_ID)
            .expect("PumpSwap program ID constant must be a valid pubkey")
    })
}

fn source_account_kind(owner_or_program: Option<Pubkey>) -> AccountSourceAccountKindV1 {
    match owner_or_program {
        Some(owner) if owner == pump_fun_program_id() => {
            AccountSourceAccountKindV1::PumpFunBondingCurve
        }
        Some(owner) if owner == pump_swap_program_id() => AccountSourceAccountKindV1::PumpSwapPool,
        Some(_) | None => AccountSourceAccountKindV1::Unknown,
    }
}

fn decision_for_primary_relation(
    relation: VersionRelation,
    agreement: AccountProviderAgreementV1,
    version: AccountMutationVersionV1,
    data_hash_blake3: [u8; 32],
) -> AccountObservationDecisionV1 {
    match relation {
        VersionRelation::Newer => AccountObservationDecisionV1 {
            classification: AccountObservationClassificationV1::NewerMutation,
            outcome: AccountObservationOutcomeV1::AppliedNewMutation,
            canonical_apply: true,
            provider_agreement: agreement,
            mutation_version: Some(version),
            data_hash_blake3: Some(data_hash_blake3),
        },
        VersionRelation::Older => AccountObservationDecisionV1 {
            classification: AccountObservationClassificationV1::OlderObservation,
            outcome: AccountObservationOutcomeV1::StaleObservation,
            canonical_apply: false,
            provider_agreement: agreement,
            mutation_version: Some(version),
            data_hash_blake3: Some(data_hash_blake3),
        },
        VersionRelation::Same => AccountObservationDecisionV1 {
            classification: AccountObservationClassificationV1::SameVersionSameHash,
            outcome: AccountObservationOutcomeV1::DuplicateObservation,
            canonical_apply: false,
            provider_agreement: agreement,
            mutation_version: Some(version),
            data_hash_blake3: Some(data_hash_blake3),
        },
        VersionRelation::WriteVersionUnknown => AccountObservationDecisionV1 {
            classification: AccountObservationClassificationV1::WriteVersionUnknown,
            outcome: AccountObservationOutcomeV1::UnorderableWithoutWriteVersion,
            canonical_apply: false,
            provider_agreement: agreement,
            mutation_version: Some(version),
            data_hash_blake3: Some(data_hash_blake3),
        },
    }
}

fn relation_to_latest(
    incoming: &AccountMutationVersionV1,
    latest: Option<&AccountObservationIdentityV1>,
) -> VersionRelation {
    let Some(latest) = latest else {
        return VersionRelation::Newer;
    };
    let latest = &latest.mutation_version;
    match incoming.slot.cmp(&latest.slot) {
        std::cmp::Ordering::Greater => VersionRelation::Newer,
        std::cmp::Ordering::Less => VersionRelation::Older,
        std::cmp::Ordering::Equal => match (incoming.write_version, latest.write_version) {
            (Some(incoming), Some(latest)) => match incoming.cmp(&latest) {
                std::cmp::Ordering::Greater => VersionRelation::Newer,
                std::cmp::Ordering::Less => VersionRelation::Older,
                std::cmp::Ordering::Equal => VersionRelation::Same,
            },
            (None, None) => VersionRelation::Same,
            (None, Some(_)) | (Some(_), None) => VersionRelation::WriteVersionUnknown,
        },
    }
}

fn parse_blake3_hex(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut out = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        out[index] = (high << 4) | low;
    }
    Some(out)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CurveFinality;

    fn hash(byte: u8) -> String {
        std::iter::repeat(format!("{byte:02x}")).take(32).collect()
    }

    fn update(
        account: Pubkey,
        slot: u64,
        write_version: Option<u64>,
        hash_byte: u8,
        provider_id: &str,
        provider_role: RawProviderRoleV1,
    ) -> AccountStateUpdate {
        AccountStateUpdate {
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            bonding_curve: account,
            sol_reserves: 1,
            token_reserves: 2,
            is_complete: 0,
            slot,
            write_version,
            source_account_pubkey: Some(account),
            source_account_owner_or_program: Some(Pubkey::new_unique()),
            account_data_len: Some(56),
            account_data_hash: Some(hash(hash_byte)),
            receive_ts_ms: 1_000,
            receive_seq: 1,
            curve_finality: CurveFinality::Finalized,
            source: UpdateSource::GeyserAccountUpdate,
            provider_id: Some(provider_id.to_owned()),
            provider_role: Some(provider_role),
            txn_signature: Some(Signature::new_unique()),
        }
    }

    #[test]
    fn receive_sequence_never_advances_same_version() {
        let account = Pubkey::new_unique();
        let mut arbiter = AccountObservationArbiter::default();
        let first = update(
            account,
            10,
            Some(1),
            1,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let mut replay = first.clone();
        replay.receive_seq = 999;

        assert!(arbiter.arbitrate(&first).canonical_apply);
        let replay_decision = arbiter.arbitrate(&replay);
        assert_eq!(
            replay_decision.classification,
            AccountObservationClassificationV1::ExactDuplicate
        );
        assert!(!replay_decision.canonical_apply);
    }

    #[test]
    fn secondary_first_is_witness_only_until_matching_primary_arrives() {
        let account = Pubkey::new_unique();
        let mut arbiter = AccountObservationArbiter::default();
        let secondary = update(
            account,
            10,
            Some(1),
            1,
            "secondary",
            RawProviderRoleV1::SecondaryWitness,
        );
        let signature = secondary.txn_signature;
        let mut primary = secondary.clone();
        primary.provider_id = Some("primary".to_owned());
        primary.provider_role = Some(RawProviderRoleV1::PrimaryAuthority);
        primary.txn_signature = signature;

        let secondary_decision = arbiter.arbitrate(&secondary);
        assert_eq!(
            secondary_decision.outcome,
            AccountObservationOutcomeV1::SecondaryWitnessRecorded
        );
        assert!(!secondary_decision.canonical_apply);

        let primary_decision = arbiter.arbitrate(&primary);
        assert!(primary_decision.canonical_apply);
        assert_eq!(
            primary_decision.provider_agreement,
            AccountProviderAgreementV1::PrimarySecondaryAgreement
        );
    }

    #[test]
    fn secondary_first_conflict_preserves_evidence_without_vetoing_primary() {
        let account = Pubkey::new_unique();
        let mut arbiter = AccountObservationArbiter::default();
        let secondary = update(
            account,
            10,
            Some(1),
            1,
            "secondary",
            RawProviderRoleV1::SecondaryWitness,
        );
        let mut primary = secondary.clone();
        primary.provider_id = Some("primary".to_owned());
        primary.provider_role = Some(RawProviderRoleV1::PrimaryAuthority);
        primary.account_data_hash = Some(hash(2));

        assert!(!arbiter.arbitrate(&secondary).canonical_apply);
        let decision = arbiter.arbitrate(&primary);
        assert_eq!(
            decision.classification,
            AccountObservationClassificationV1::NewerMutation
        );
        assert_eq!(
            decision.outcome,
            AccountObservationOutcomeV1::AppliedNewMutation
        );
        assert!(decision.canonical_apply);
        assert_eq!(
            decision.provider_agreement,
            AccountProviderAgreementV1::PrimarySecondaryConflict
        );
        let snapshot = arbiter.snapshot();
        assert_eq!(
            snapshot.latest_primary_canonical.unwrap().data_hash_blake3,
            [2; 32]
        );
        assert_eq!(snapshot.conflicts.len(), 1);
        assert_eq!(snapshot.conflicts[0].observations.len(), 2);
    }

    #[test]
    fn none_and_zero_write_versions_are_never_equated() {
        let account = Pubkey::new_unique();
        let mut arbiter = AccountObservationArbiter::default();
        let zero = update(
            account,
            10,
            Some(0),
            1,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let unknown = update(
            account,
            10,
            None,
            2,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );

        assert!(arbiter.arbitrate(&zero).canonical_apply);
        let decision = arbiter.arbitrate(&unknown);
        assert_eq!(
            decision.classification,
            AccountObservationClassificationV1::WriteVersionUnknown
        );
        assert!(!decision.canonical_apply);
    }

    #[test]
    fn conflict_evidence_keeps_every_witness_and_absent_signature() {
        let account = Pubkey::new_unique();
        let mut arbiter = AccountObservationArbiter::default();
        let mut primary = update(
            account,
            10,
            Some(1),
            1,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        primary.txn_signature = None;

        let mut agreeing_secondary = primary.clone();
        agreeing_secondary.provider_id = Some("secondary".to_owned());
        agreeing_secondary.provider_role = Some(RawProviderRoleV1::SecondaryWitness);

        let mut conflicting_secondary = agreeing_secondary.clone();
        conflicting_secondary.account_data_hash = Some(hash(2));

        let mut primary_replay = primary.clone();
        primary_replay.receive_seq = 99;

        assert!(arbiter.arbitrate(&primary).canonical_apply);
        let agreement = arbiter.arbitrate(&agreeing_secondary);
        assert_eq!(
            agreement.provider_agreement,
            AccountProviderAgreementV1::PrimarySecondaryAgreement
        );
        let conflict = arbiter.arbitrate(&conflicting_secondary);
        assert_eq!(
            conflict.classification,
            AccountObservationClassificationV1::SameVersionDifferentHashConflict
        );
        assert_eq!(
            conflict.provider_agreement,
            AccountProviderAgreementV1::PrimarySecondaryConflict
        );
        let replay_after_conflict = arbiter.arbitrate(&primary_replay);
        assert_eq!(
            replay_after_conflict.classification,
            AccountObservationClassificationV1::ExactDuplicate
        );

        let snapshot = arbiter.snapshot();
        assert_eq!(snapshot.conflicts.len(), 1);
        assert_eq!(snapshot.conflicts[0].observations.len(), 3);
        assert_eq!(
            snapshot.conflicts[0]
                .observations
                .iter()
                .filter(|evidence| evidence.provider_id == "secondary")
                .count(),
            2,
            "the pre-conflict agreement witness and the conflicting witness must both survive"
        );
        assert!(snapshot.conflicts[0]
            .observations
            .iter()
            .all(|evidence| evidence.txn_signature.is_none()));
    }

    #[test]
    fn provider_observation_identity_ignores_receive_metadata() {
        let account = Pubkey::new_unique();
        let first = update(
            account,
            10,
            Some(1),
            1,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let mut replay = first.clone();
        replay.receive_ts_ms = replay.receive_ts_ms.saturating_add(500);
        replay.receive_seq = replay.receive_seq.saturating_add(100);

        let first = AccountObservationV1::from_update(&first).expect("valid test observation");
        let replay =
            AccountObservationV1::from_update(&replay).expect("valid replay test observation");
        assert_eq!(
            first.evidence.provider_observation_identity(),
            replay.evidence.provider_observation_identity(),
            "receive metadata must not redefine a chain observation"
        );
    }

    #[test]
    fn secondary_evidence_overflow_keeps_the_first_rejected_observation() {
        let account = Pubkey::new_unique();
        let mut arbiter =
            AccountObservationArbiter::with_limits(AccountObservationArbiterLimitsV1 {
                max_versions_per_account: 2,
                max_unique_observations_per_version: 1,
                max_identity_conflicts_per_account: 1,
                max_identity_transitions_per_account: 1,
            });
        let primary = update(
            account,
            10,
            Some(1),
            1,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let mut retained_secondary_witness = primary.clone();
        retained_secondary_witness.provider_id = Some("secondary-1".to_owned());
        retained_secondary_witness.provider_role = Some(RawProviderRoleV1::SecondaryWitness);
        retained_secondary_witness.txn_signature = Some(Signature::new_unique());

        let mut rejected_secondary_witness = retained_secondary_witness.clone();
        rejected_secondary_witness.provider_id = Some("secondary-2".to_owned());
        rejected_secondary_witness.txn_signature = Some(Signature::new_unique());
        rejected_secondary_witness.account_data_hash = Some(hash(9));

        assert!(arbiter.arbitrate(&primary).canonical_apply);
        assert_eq!(
            arbiter.arbitrate(&retained_secondary_witness).outcome,
            AccountObservationOutcomeV1::SecondaryWitnessRecorded
        );
        let overflow = arbiter.arbitrate(&rejected_secondary_witness);
        assert_eq!(
            overflow.classification,
            AccountObservationClassificationV1::EvidenceCapacityExceeded
        );
        assert_eq!(
            overflow.outcome,
            AccountObservationOutcomeV1::EvidenceCapacityExceeded
        );
        assert!(!overflow.canonical_apply);
        let snapshot = arbiter.snapshot();
        assert_eq!(snapshot.counters.evidence_capacity_exceeded_count, 1);
        assert!(!snapshot.secondary_evidence_complete);
        let first_overflow = snapshot
            .first_evidence_overflow
            .expect("overflow must remain visible");
        assert_eq!(
            first_overflow.scope,
            AccountObservationEvidenceOverflowScopeV1::VersionObservations
        );
        let rejected = first_overflow
            .first_rejected_observation
            .expect("the first rejected observation must retain full provenance");
        assert_eq!(rejected.provider_id, "secondary-2");
        assert_eq!(rejected.provider_role, RawProviderRoleV1::SecondaryWitness);
        assert_eq!(
            rejected.txn_signature,
            rejected_secondary_witness.txn_signature
        );
        assert_eq!(rejected.identity.data_hash_blake3, [9; 32]);
        assert_eq!(overflow.data_hash_blake3, Some([9; 32]));
        assert!(snapshot.conflicts.is_empty());
    }

    #[test]
    fn saturated_secondary_version_evidence_never_vetoes_later_primary() {
        let account = Pubkey::new_unique();
        let mut arbiter =
            AccountObservationArbiter::with_limits(AccountObservationArbiterLimitsV1 {
                max_versions_per_account: 2,
                max_unique_observations_per_version: 2,
                max_identity_conflicts_per_account: 1,
                max_identity_transitions_per_account: 1,
            });
        let secondary_one = update(
            account,
            10,
            Some(1),
            1,
            "secondary-1",
            RawProviderRoleV1::SecondaryWitness,
        );
        let mut secondary_two = secondary_one.clone();
        secondary_two.provider_id = Some("secondary-2".to_owned());
        secondary_two.txn_signature = Some(Signature::new_unique());
        secondary_two.account_data_hash = Some(hash(2));
        let mut overflow_secondary = secondary_two.clone();
        overflow_secondary.provider_id = Some("secondary-3".to_owned());
        overflow_secondary.txn_signature = Some(Signature::new_unique());
        overflow_secondary.account_data_hash = Some(hash(3));

        assert_eq!(
            arbiter.arbitrate(&secondary_one).outcome,
            AccountObservationOutcomeV1::SecondaryWitnessRecorded
        );
        assert_eq!(
            arbiter.arbitrate(&secondary_two).outcome,
            AccountObservationOutcomeV1::ProviderConflict
        );
        assert_eq!(
            arbiter.arbitrate(&overflow_secondary).classification,
            AccountObservationClassificationV1::EvidenceCapacityExceeded
        );

        let primary = update(
            account,
            10,
            Some(1),
            4,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let primary_decision = arbiter.arbitrate(&primary);
        assert_eq!(
            primary_decision.outcome,
            AccountObservationOutcomeV1::AppliedNewMutation
        );
        assert!(primary_decision.canonical_apply);

        let primary_replay = arbiter.arbitrate(&primary);
        assert!(!primary_replay.canonical_apply);
        assert_eq!(
            arbiter.snapshot().counters.canonical_mutation_count,
            1,
            "a secondary-saturated version may never prevent or duplicate the primary mutation"
        );
    }

    #[test]
    fn saturated_secondary_version_index_never_vetoes_newer_primary() {
        let account = Pubkey::new_unique();
        let mut arbiter =
            AccountObservationArbiter::with_limits(AccountObservationArbiterLimitsV1 {
                max_versions_per_account: 2,
                max_unique_observations_per_version: 2,
                max_identity_conflicts_per_account: 1,
                max_identity_transitions_per_account: 1,
            });

        for (slot, hash_byte) in [(10, 1), (11, 2)] {
            let secondary = update(
                account,
                slot,
                Some(slot),
                hash_byte,
                "secondary",
                RawProviderRoleV1::SecondaryWitness,
            );
            assert_eq!(
                arbiter.arbitrate(&secondary).outcome,
                AccountObservationOutcomeV1::SecondaryWitnessRecorded
            );
        }

        let rejected_secondary = update(
            account,
            12,
            Some(12),
            3,
            "secondary",
            RawProviderRoleV1::SecondaryWitness,
        );
        assert_eq!(
            arbiter.arbitrate(&rejected_secondary).classification,
            AccountObservationClassificationV1::EvidenceCapacityExceeded
        );

        let primary = update(
            account,
            13,
            Some(13),
            4,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let primary_decision = arbiter.arbitrate(&primary);
        assert_eq!(
            primary_decision.outcome,
            AccountObservationOutcomeV1::AppliedNewMutation
        );
        assert!(primary_decision.canonical_apply);

        let snapshot = arbiter.snapshot();
        assert!(!snapshot.secondary_evidence_complete);
        assert_eq!(
            snapshot
                .latest_primary_canonical
                .expect("newer primary must establish canonical watermark")
                .mutation_version
                .slot,
            13
        );
    }

    #[test]
    fn secondary_conflicts_cannot_poison_primary_watermark_rotation() {
        let account = Pubkey::new_unique();
        let mut arbiter =
            AccountObservationArbiter::with_limits(AccountObservationArbiterLimitsV1 {
                max_versions_per_account: 2,
                max_unique_observations_per_version: 2,
                max_identity_conflicts_per_account: 1,
                max_identity_transitions_per_account: 1,
            });

        let primary_v1 = update(
            account,
            10,
            Some(1),
            1,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let mut secondary_v1 = primary_v1.clone();
        secondary_v1.provider_id = Some("secondary".to_owned());
        secondary_v1.provider_role = Some(RawProviderRoleV1::SecondaryWitness);
        secondary_v1.account_data_hash = Some(hash(2));

        let primary_v2 = update(
            account,
            11,
            Some(2),
            3,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let mut secondary_v2 = primary_v2.clone();
        secondary_v2.provider_id = Some("secondary".to_owned());
        secondary_v2.provider_role = Some(RawProviderRoleV1::SecondaryWitness);
        secondary_v2.account_data_hash = Some(hash(4));

        assert!(arbiter.arbitrate(&primary_v1).canonical_apply);
        assert_eq!(
            arbiter.arbitrate(&secondary_v1).outcome,
            AccountObservationOutcomeV1::ProviderConflict
        );
        assert!(arbiter.arbitrate(&primary_v2).canonical_apply);
        assert_eq!(
            arbiter.arbitrate(&secondary_v2).outcome,
            AccountObservationOutcomeV1::ProviderConflict
        );

        let secondary_v3_first = update(
            account,
            12,
            Some(3),
            5,
            "secondary-1",
            RawProviderRoleV1::SecondaryWitness,
        );
        let mut secondary_v3_conflict = secondary_v3_first.clone();
        secondary_v3_conflict.provider_id = Some("secondary-2".to_owned());
        secondary_v3_conflict.account_data_hash = Some(hash(6));
        assert_eq!(
            arbiter.arbitrate(&secondary_v3_first).outcome,
            AccountObservationOutcomeV1::SecondaryWitnessRecorded
        );
        let secondary_overflow = arbiter.arbitrate(&secondary_v3_conflict);
        assert_eq!(
            secondary_overflow.classification,
            AccountObservationClassificationV1::EvidenceCapacityExceeded,
            "the full conflict store must degrade only secondary evidence"
        );

        let primary_v3 = update(
            account,
            12,
            Some(3),
            7,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let primary_v3_decision = arbiter.arbitrate(&primary_v3);
        assert_eq!(
            primary_v3_decision.outcome,
            AccountObservationOutcomeV1::AppliedNewMutation
        );
        assert!(
            primary_v3_decision.canonical_apply,
            "secondary conflicts for old versions must never veto a newer eligible primary"
        );

        let snapshot = arbiter.snapshot();
        assert_eq!(snapshot.counters.canonical_mutation_count, 3);
        assert_eq!(snapshot.conflicts.len(), 2);
        assert!(
            !snapshot.secondary_evidence_complete,
            "overflowed witness conflict evidence must be explicit rather than vetoing primary"
        );
        assert_eq!(
            snapshot
                .conflicts
                .iter()
                .map(|conflict| conflict.mutation_version.slot)
                .collect::<Vec<_>>(),
            vec![10, 11],
            "pruned ordering records must leave their bounded conflict evidence behind"
        );
        assert_eq!(snapshot.counters.pruned_non_conflict_version_count, 1);
        let first_overflow = snapshot
            .first_evidence_overflow
            .expect("conflict-store overflow must be retained");
        assert_eq!(
            first_overflow.scope,
            AccountObservationEvidenceOverflowScopeV1::ProviderConflictEvidence
        );
        assert_eq!(
            first_overflow
                .first_rejected_observation
                .expect("overflow must retain the rejected secondary witness")
                .identity
                .data_hash_blake3,
            [6; 32]
        );
    }

    #[test]
    fn stale_primary_correlates_with_secondary_conflict_without_authority_capacity() {
        let account = Pubkey::new_unique();
        let mut arbiter =
            AccountObservationArbiter::with_limits(AccountObservationArbiterLimitsV1 {
                max_versions_per_account: 2,
                max_unique_observations_per_version: 2,
                max_identity_conflicts_per_account: 1,
                max_identity_transitions_per_account: 1,
            });

        let primary_v10 = update(
            account,
            10,
            Some(10),
            1,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        assert!(arbiter.arbitrate(&primary_v10).canonical_apply);

        let secondary_v9 = update(
            account,
            9,
            Some(9),
            2,
            "secondary",
            RawProviderRoleV1::SecondaryWitness,
        );
        assert_eq!(
            arbiter.arbitrate(&secondary_v9).outcome,
            AccountObservationOutcomeV1::SecondaryWitnessRecorded
        );

        let primary_v9 = update(
            account,
            9,
            Some(9),
            3,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let conflict = arbiter.arbitrate(&primary_v9);
        assert_eq!(
            conflict.classification,
            AccountObservationClassificationV1::SameVersionDifferentHashConflict
        );
        assert_eq!(
            conflict.outcome,
            AccountObservationOutcomeV1::ProviderConflict
        );
        assert_eq!(
            conflict.provider_agreement,
            AccountProviderAgreementV1::PrimarySecondaryConflict
        );
        assert!(!conflict.canonical_apply);
        assert_eq!(
            arbiter.primary_versions.len(),
            1,
            "a stale primary correlation must not consume primary ordering capacity"
        );

        let primary_v11 = update(
            account,
            11,
            Some(11),
            4,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        assert!(arbiter.arbitrate(&primary_v11).canonical_apply);

        let snapshot = arbiter.snapshot();
        assert_eq!(snapshot.counters.canonical_mutation_count, 2);
        assert_eq!(snapshot.conflicts.len(), 1);
        assert_eq!(snapshot.conflicts[0].mutation_version.slot, 9);
        assert_eq!(
            snapshot
                .latest_primary_canonical
                .expect("V11 must become the latest canonical primary mutation")
                .mutation_version
                .slot,
            11
        );
    }

    #[test]
    fn stale_primary_correlates_with_secondary_agreement_without_canonical_apply() {
        let account = Pubkey::new_unique();
        let mut arbiter =
            AccountObservationArbiter::with_limits(AccountObservationArbiterLimitsV1 {
                max_versions_per_account: 2,
                max_unique_observations_per_version: 2,
                max_identity_conflicts_per_account: 1,
                max_identity_transitions_per_account: 1,
            });

        let primary_v10 = update(
            account,
            10,
            Some(10),
            1,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        assert!(arbiter.arbitrate(&primary_v10).canonical_apply);

        let secondary_v9 = update(
            account,
            9,
            Some(9),
            2,
            "secondary",
            RawProviderRoleV1::SecondaryWitness,
        );
        assert_eq!(
            arbiter.arbitrate(&secondary_v9).outcome,
            AccountObservationOutcomeV1::SecondaryWitnessRecorded
        );

        let primary_v9_same_hash = update(
            account,
            9,
            Some(9),
            2,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let agreement = arbiter.arbitrate(&primary_v9_same_hash);
        assert_eq!(
            agreement.classification,
            AccountObservationClassificationV1::SameVersionSameHash
        );
        assert_eq!(
            agreement.outcome,
            AccountObservationOutcomeV1::DuplicateObservation
        );
        assert_eq!(
            agreement.provider_agreement,
            AccountProviderAgreementV1::PrimarySecondaryAgreement
        );
        assert!(!agreement.canonical_apply);
        assert_eq!(arbiter.primary_versions.len(), 1);
        assert!(arbiter.snapshot().conflicts.is_empty());

        let primary_v11 = update(
            account,
            11,
            Some(11),
            3,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        assert!(arbiter.arbitrate(&primary_v11).canonical_apply);
        let snapshot = arbiter.snapshot();
        assert_eq!(snapshot.counters.canonical_mutation_count, 2);
        assert_eq!(
            snapshot
                .latest_primary_canonical
                .expect("V11 must become the latest canonical primary mutation")
                .mutation_version
                .slot,
            11
        );
    }

    #[test]
    fn unorderable_primary_correlates_with_secondary_without_authority_capacity() {
        let account = Pubkey::new_unique();
        let mut arbiter =
            AccountObservationArbiter::with_limits(AccountObservationArbiterLimitsV1 {
                max_versions_per_account: 2,
                max_unique_observations_per_version: 2,
                max_identity_conflicts_per_account: 1,
                max_identity_transitions_per_account: 1,
            });

        let primary_known = update(
            account,
            10,
            Some(10),
            1,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        assert!(arbiter.arbitrate(&primary_known).canonical_apply);

        let secondary_unknown = update(
            account,
            10,
            None,
            2,
            "secondary",
            RawProviderRoleV1::SecondaryWitness,
        );
        assert_eq!(
            arbiter.arbitrate(&secondary_unknown).classification,
            AccountObservationClassificationV1::WriteVersionUnknown
        );

        let primary_unknown_same_hash = update(
            account,
            10,
            None,
            2,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let agreement = arbiter.arbitrate(&primary_unknown_same_hash);
        assert_eq!(
            agreement.classification,
            AccountObservationClassificationV1::SameVersionSameHash
        );
        assert_eq!(
            agreement.provider_agreement,
            AccountProviderAgreementV1::PrimarySecondaryAgreement
        );
        assert!(!agreement.canonical_apply);
        assert_eq!(arbiter.primary_versions.len(), 1);
        assert_eq!(arbiter.snapshot().counters.canonical_mutation_count, 1);
    }

    #[test]
    fn stale_primary_replays_never_consume_primary_watermark_capacity() {
        let account = Pubkey::new_unique();
        let mut arbiter =
            AccountObservationArbiter::with_limits(AccountObservationArbiterLimitsV1 {
                max_versions_per_account: 2,
                max_unique_observations_per_version: 2,
                max_identity_conflicts_per_account: 1,
                max_identity_transitions_per_account: 1,
            });

        let primary_v10 = update(
            account,
            10,
            Some(10),
            10,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        assert!(arbiter.arbitrate(&primary_v10).canonical_apply);

        for stale_slot in 1..10 {
            let stale = update(
                account,
                stale_slot,
                Some(stale_slot),
                stale_slot as u8,
                "primary",
                RawProviderRoleV1::PrimaryAuthority,
            );
            let decision = arbiter.arbitrate(&stale);
            assert_eq!(
                decision.classification,
                AccountObservationClassificationV1::OlderObservation
            );
            assert!(!decision.canonical_apply);
        }

        let primary_v11 = update(
            account,
            11,
            Some(11),
            11,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let applied = arbiter.arbitrate(&primary_v11);
        assert_eq!(
            applied.outcome,
            AccountObservationOutcomeV1::AppliedNewMutation
        );
        assert!(applied.canonical_apply);

        let snapshot = arbiter.snapshot();
        assert_eq!(snapshot.counters.canonical_mutation_count, 2);
        assert_eq!(snapshot.counters.stale_observation_count, 9);
        assert_eq!(
            snapshot
                .latest_primary_canonical
                .expect("newer primary must advance the canonical watermark")
                .mutation_version
                .slot,
            11
        );
        assert!(
            snapshot.first_evidence_overflow.is_none(),
            "stale primary replays are classified no-ops, not authority-lane overflow"
        );
    }

    #[test]
    fn primary_conflict_store_overflow_degrades_evidence_without_veto() {
        let account = Pubkey::new_unique();
        let mut arbiter =
            AccountObservationArbiter::with_limits(AccountObservationArbiterLimitsV1 {
                max_versions_per_account: 2,
                max_unique_observations_per_version: 2,
                max_identity_conflicts_per_account: 1,
                max_identity_transitions_per_account: 1,
            });

        for (slot, primary_hash, secondary_hash) in [(10, 1, 2), (11, 3, 4)] {
            let primary = update(
                account,
                slot,
                Some(slot),
                primary_hash,
                "primary",
                RawProviderRoleV1::PrimaryAuthority,
            );
            let mut secondary = primary.clone();
            secondary.provider_id = Some("secondary".to_owned());
            secondary.provider_role = Some(RawProviderRoleV1::SecondaryWitness);
            secondary.account_data_hash = Some(hash(secondary_hash));

            assert!(arbiter.arbitrate(&primary).canonical_apply);
            assert_eq!(
                arbiter.arbitrate(&secondary).outcome,
                AccountObservationOutcomeV1::ProviderConflict
            );
        }
        assert_eq!(arbiter.snapshot().conflicts.len(), 2);

        let secondary_v3 = update(
            account,
            12,
            Some(12),
            5,
            "secondary",
            RawProviderRoleV1::SecondaryWitness,
        );
        assert_eq!(
            arbiter.arbitrate(&secondary_v3).outcome,
            AccountObservationOutcomeV1::SecondaryWitnessRecorded
        );

        let primary_v3 = update(
            account,
            12,
            Some(12),
            6,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let decision = arbiter.arbitrate(&primary_v3);
        assert_eq!(
            decision.outcome,
            AccountObservationOutcomeV1::AppliedNewMutation
        );
        assert!(
            decision.canonical_apply,
            "a full conflict-evidence store must not veto an eligible primary"
        );

        let snapshot = arbiter.snapshot();
        assert_eq!(snapshot.counters.canonical_mutation_count, 3);
        assert_eq!(snapshot.counters.evidence_capacity_exceeded_count, 1);
        assert!(
            !snapshot.secondary_evidence_complete,
            "unretained secondary/primary conflict evidence must be explicit"
        );
        let overflow = snapshot
            .first_evidence_overflow
            .expect("the full conflict store must retain an overflow marker");
        assert_eq!(
            overflow.scope,
            AccountObservationEvidenceOverflowScopeV1::ProviderConflictEvidence
        );
        assert_eq!(
            overflow
                .first_rejected_observation
                .expect("overflow must retain the first unretained conflict observation")
                .identity
                .data_hash_blake3,
            [6; 32]
        );
    }

    #[test]
    fn primary_capacity_overflow_retains_rejected_primary_provenance() {
        let account = Pubkey::new_unique();
        let mut arbiter =
            AccountObservationArbiter::with_limits(AccountObservationArbiterLimitsV1 {
                max_versions_per_account: 1,
                max_unique_observations_per_version: 1,
                max_identity_conflicts_per_account: 1,
                max_identity_transitions_per_account: 1,
            });
        let first = update(
            account,
            10,
            Some(1),
            1,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let rejected = update(
            account,
            11,
            Some(2),
            2,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );

        assert!(arbiter.arbitrate(&first).canonical_apply);
        assert_eq!(
            arbiter.arbitrate(&rejected).classification,
            AccountObservationClassificationV1::EvidenceCapacityExceeded
        );

        let snapshot = arbiter.snapshot();
        assert!(
            snapshot.secondary_evidence_complete,
            "a primary-lane capacity failure does not claim that witness evidence was lost"
        );
        let rejected_evidence = snapshot
            .first_evidence_overflow
            .expect("primary overflow must retain an audit record")
            .first_rejected_observation
            .expect("primary overflow must retain the rejected observation itself");
        assert_eq!(rejected_evidence.provider_id, "primary");
        assert_eq!(
            rejected_evidence.provider_role,
            RawProviderRoleV1::PrimaryAuthority
        );
        assert_eq!(rejected_evidence.identity.data_hash_blake3, [2; 32]);
        assert_eq!(rejected_evidence.txn_signature, rejected.txn_signature);
    }

    #[test]
    fn exactly_once_is_explicitly_in_process_until_pr1d_persists_watermarks() {
        let account = Pubkey::new_unique();
        let primary = update(
            account,
            10,
            Some(1),
            1,
            "primary",
            RawProviderRoleV1::PrimaryAuthority,
        );
        let mut first_process = AccountObservationArbiter::default();
        assert!(first_process.arbitrate(&primary).canonical_apply);

        let mut restarted_process = AccountObservationArbiter::default();
        let replay_after_restart = restarted_process.arbitrate(&primary);
        assert!(replay_after_restart.canonical_apply);
        assert_eq!(
            replay_after_restart.outcome,
            AccountObservationOutcomeV1::AppliedNewMutation,
            "PR1C promises exactly-once only within one in-memory arbiter; durable restart reconciliation belongs to PR1D"
        );
    }

    #[test]
    fn old_arbiter_counter_json_defaults_bounded_evidence_fields() {
        let counters: AccountObservationArbiterCountersV1 = serde_json::from_str(
            r#"{
                "provider_observation_count": 1,
                "canonical_mutation_count": 2,
                "duplicate_observation_count": 3,
                "stale_observation_count": 4,
                "provider_conflict_count": 5,
                "unorderable_observation_count": 6,
                "secondary_witness_count": 7,
                "invalid_observation_count": 8
            }"#,
        )
        .expect("pre-bounded-evidence counter JSON remains readable");

        assert_eq!(counters.provider_observation_count, 1);
        assert_eq!(counters.invalid_observation_count, 8);
        assert_eq!(counters.evidence_capacity_exceeded_count, 0);
        assert_eq!(counters.pruned_non_conflict_version_count, 0);
    }

    #[test]
    fn old_arbiter_snapshot_json_defaults_secondary_evidence_to_complete() {
        let snapshot: AccountObservationArbiterSnapshotV1 =
            serde_json::from_str("{}").expect("pre-secondary-lane snapshot JSON remains readable");
        assert!(snapshot.secondary_evidence_complete);
        assert!(snapshot.first_evidence_overflow.is_none());

        let overflow: AccountObservationEvidenceOverflowV1 = serde_json::from_str(
            r#"{
                "scope": "version_observations",
                "retained_count": 32,
                "overflow_count": 1
            }"#,
        )
        .expect("pre-provenance overflow JSON remains readable");
        assert!(overflow.first_rejected_observation.is_none());
    }
}
