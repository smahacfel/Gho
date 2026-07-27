//! Bounded, synchronous reconciliation of primary raw Pump mutations and
//! witness-only observations.
//!
//! The ledger owns no runtime task, performs no I/O and never waits for a
//! witness before returning an eligible primary structural mutation.  It is a
//! classification boundary, not a quote engine and not a Gatekeeper policy.

use crate::ingest_integrity::{
    CandidateIntegrityOutcomeV1, CandidateIntegritySignalV1, ObservationProvenanceV1,
    ObservationSourceFamilyV1, ObservedPumpMutationV1, ParsedWitnessCorrelationOutcomeV1,
    PumpCandidateIdentityV1, PumpEconomicCertificationStatusV1, PumpMutationConflictFieldV1,
    PumpProviderAgreementV1, RawProviderRoleV1, RawPumpMutationLocatorV1,
    StructuralCanonicalPumpMutationV1,
};
use serde::{Deserialize, Serialize};
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet};
use thiserror::Error;

/// Bounded capacities and deterministic correlation retention.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpObservationLedgerConfigV1 {
    /// A locatorless parsed witness is finalized only after this interval.
    pub correlation_window_ns: u64,
    /// Capacity reserved exclusively for canonical primary raw mutations.
    pub max_primary_canonical_mutations: usize,
    /// Shared witness-only pending lane. Saturation never consumes primary
    /// capacity and never vetoes an eligible primary.
    pub max_pending_witnesses: usize,
    /// Correlated provider observations retained per canonical mutation.
    pub max_correlated_witnesses_per_mutation: usize,
    /// Retained material conflicts. Classification continues after saturation,
    /// while evidence completeness is marked false.
    pub max_retained_conflicts: usize,
}

impl Default for PumpObservationLedgerConfigV1 {
    fn default() -> Self {
        Self {
            correlation_window_ns: 250_000_000,
            max_primary_canonical_mutations: 131_072,
            max_pending_witnesses: 16_384,
            max_correlated_witnesses_per_mutation: 32,
            max_retained_conflicts: 8_192,
        }
    }
}

impl PumpObservationLedgerConfigV1 {
    fn validate(self) -> Result<Self, PumpObservationLedgerConfigErrorV1> {
        if self.correlation_window_ns == 0 {
            return Err(PumpObservationLedgerConfigErrorV1::ZeroCorrelationWindow);
        }
        if self.max_primary_canonical_mutations == 0 {
            return Err(PumpObservationLedgerConfigErrorV1::ZeroCapacity(
                PumpObservationEvidenceLaneV1::PrimaryCanonical,
            ));
        }
        if self.max_pending_witnesses == 0 {
            return Err(PumpObservationLedgerConfigErrorV1::ZeroCapacity(
                PumpObservationEvidenceLaneV1::PendingWitness,
            ));
        }
        if self.max_correlated_witnesses_per_mutation == 0 {
            return Err(PumpObservationLedgerConfigErrorV1::ZeroCapacity(
                PumpObservationEvidenceLaneV1::CorrelatedWitness,
            ));
        }
        if self.max_retained_conflicts == 0 {
            return Err(PumpObservationLedgerConfigErrorV1::ZeroCapacity(
                PumpObservationEvidenceLaneV1::Conflict,
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpObservationEvidenceLaneV1 {
    PrimaryCanonical,
    PendingWitness,
    CorrelatedWitness,
    Conflict,
    ExpiredWitnessAudit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum PumpObservationLedgerConfigErrorV1 {
    #[error("Pump observation correlation window must be greater than zero")]
    ZeroCorrelationWindow,
    #[error("Pump observation evidence lane {0:?} must have non-zero capacity")]
    ZeroCapacity(PumpObservationEvidenceLaneV1),
}

/// Exactly one immediate classification assigned to each accepted observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpObservationClassificationV1 {
    PrimaryCanonicalApplied,
    /// The final expected primary mutation for a transaction arrived and all
    /// candidates represented by that inventory can now become `Ready`.
    PrimaryTransactionInventoryComplete,
    ExactDuplicate,
    SameMutationAgreement,
    SecondaryWitnessOnly,
    /// A secondary raw witness exhausted the bounded correlation window
    /// without an exact primary locator. It is audited and removed from the
    /// pending lane; it never enables singleton correlation or canonical apply.
    SecondaryWitnessExpired,
    ParsedWitnessPending,
    ExactStructuralMatch,
    UniqueSignatureSingletonMatch,
    AmbiguousParsedWitness,
    UnmatchableParsedWitness,
    SourceReconciliationConflict,
    PrimaryRawCoverageIncomplete,
    EvidenceCapacityExceeded,
}

/// First observation that could not be retained in a bounded evidence lane.
///
/// Keeping the complete immutable observation makes the overflow auditable
/// without turning the witness lane into an unbounded in-memory ledger.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpObservationEvidenceOverflowV1 {
    pub lane: PumpObservationEvidenceLaneV1,
    pub first_rejected_observation: ObservedPumpMutationV1,
    pub retained_count: usize,
    pub overflow_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpSourceConflictEvidenceV1 {
    pub locator: RawPumpMutationLocatorV1,
    pub primary_raw_provenance: ObservationProvenanceV1,
    pub witness_provenance: ObservationProvenanceV1,
    pub conflict_fields: Vec<PumpMutationConflictFieldV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation: Option<ParsedWitnessCorrelationOutcomeV1>,
}

/// One ledger decision. `canonical_mutation` being `Some` is the only
/// structural canonical-apply authority emitted by this module.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpObservationLedgerDecisionV1 {
    pub classification: PumpObservationClassificationV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation: Option<ParsedWitnessCorrelationOutcomeV1>,
    pub provider_agreement: PumpProviderAgreementV1,
    #[serde(default)]
    pub conflict_fields: Vec<PumpMutationConflictFieldV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_mutation: Option<StructuralCanonicalPumpMutationV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_integrity_signal: Option<CandidateIntegritySignalV1>,
    /// Immutable identity/provenance of a secondary witness removed by the
    /// deterministic expiry boundary. It is audit evidence only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expired_witness_observation: Option<ObservedPumpMutationV1>,
    pub evidence_complete: bool,
}

impl PumpObservationLedgerDecisionV1 {
    #[must_use]
    pub const fn did_canonical_apply(&self) -> bool {
        self.canonical_mutation.is_some()
    }
}

/// Immediate decision plus correlation decisions unlocked by the same input.
///
/// A primary raw observation can apply immediately and simultaneously resolve
/// an earlier exact witness.  Those witness outcomes are returned in
/// `derived_decisions`; the raw source is never delayed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpObservationLedgerResultV1 {
    pub observation_decision: PumpObservationLedgerDecisionV1,
    #[serde(default)]
    pub derived_decisions: Vec<PumpObservationLedgerDecisionV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpObservationLedgerSnapshotV1 {
    pub canonical_mutation_count: u64,
    pub provider_observation_count: u64,
    pub exact_duplicate_count: u64,
    pub conflict_count: u64,
    pub pending_witness_count: usize,
    pub retained_conflict_count: usize,
    /// Finalized ambiguous/unmatchable witnesses retained for exact replay
    /// detection without consuming primary canonical capacity.
    pub finalized_unassigned_witness_count: usize,
    #[serde(default)]
    pub retained_expired_witness_count: usize,
    #[serde(default)]
    pub expired_witness_audit_overflow_count: u64,
    /// Total typed evidence overflows across all bounded witness/conflict
    /// lanes. The immutable first-overflow record remains a separate audit
    /// anchor.
    pub evidence_overflow_count: u64,
    pub primary_evidence_complete: bool,
    pub witness_evidence_complete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_evidence_overflow: Option<PumpObservationEvidenceOverflowV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_rejected_expired_witness: Option<ObservedPumpMutationV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ProviderObservationIdentity {
    mutation_family: crate::ingest_integrity::PumpMutationFamilyV1,
    signature: Signature,
    locator: Option<RawPumpMutationLocatorV1>,
    source_family: ObservationSourceFamilyV1,
    source_id: String,
    provider_id: String,
    provider_role: Option<RawProviderRoleV1>,
    schema_id: String,
    payload_hash_blake3: [u8; 32],
}

impl From<&ObservedPumpMutationV1> for ProviderObservationIdentity {
    fn from(observation: &ObservedPumpMutationV1) -> Self {
        Self {
            mutation_family: observation.mutation_family,
            signature: observation.signature,
            locator: observation.locator_hint.clone(),
            source_family: observation.provenance.source_family,
            source_id: observation.provenance.source_id.clone(),
            provider_id: observation.provenance.provider_id.clone(),
            provider_role: observation.raw_provider_role,
            schema_id: observation.provenance.schema_id.clone(),
            payload_hash_blake3: observation.provenance.payload_hash_blake3,
        }
    }
}

#[derive(Clone, Debug)]
struct CanonicalRecord {
    canonical: StructuralCanonicalPumpMutationV1,
    candidate: Option<PumpCandidateIdentityV1>,
    primary_observation: ObservedPumpMutationV1,
    raw_transaction_mutation_count: Option<u32>,
    observation_identities: HashSet<ProviderObservationIdentity>,
    correlated_witnesses: Vec<ObservedPumpMutationV1>,
}

#[derive(Clone, Debug)]
struct PendingWitness {
    observation: ObservedPumpMutationV1,
    identity: ProviderObservationIdentity,
    first_seen_monotonic_ns: u64,
}

#[derive(Clone, Debug)]
struct FinalizedUnassignedWitness {
    observation: ObservedPumpMutationV1,
    identity: ProviderObservationIdentity,
    outcome: ParsedWitnessCorrelationOutcomeV1,
}

#[derive(Clone, Debug)]
struct PrimaryTransactionInventory {
    declared_count: u32,
    first_seen_monotonic_ns: u64,
    completion_signaled: bool,
    incomplete_signaled: bool,
}

/// Pure in-process Pump observation arbiter.
///
/// Exactly-once here is bounded to the process and retained primary lane.
/// Durable cross-restart reconstruction belongs to the later durable
/// Observation Ledger/replay work.
#[derive(Debug)]
pub struct PumpObservationLedgerV1 {
    config: PumpObservationLedgerConfigV1,
    canonical_by_locator: HashMap<RawPumpMutationLocatorV1, CanonicalRecord>,
    canonical_locators_by_signature: HashMap<Signature, Vec<RawPumpMutationLocatorV1>>,
    primary_transaction_inventories: HashMap<Signature, PrimaryTransactionInventory>,
    pending_witnesses: Vec<PendingWitness>,
    finalized_unassigned_witnesses: Vec<FinalizedUnassignedWitness>,
    expired_witness_audit: Vec<ObservedPumpMutationV1>,
    expired_witness_audit_overflow_count: u64,
    first_rejected_expired_witness: Option<ObservedPumpMutationV1>,
    retained_conflicts: Vec<PumpSourceConflictEvidenceV1>,
    first_evidence_overflow: Option<PumpObservationEvidenceOverflowV1>,
    overflow_counts: HashMap<PumpObservationEvidenceLaneV1, u64>,
    canonical_mutation_count: u64,
    provider_observation_count: u64,
    exact_duplicate_count: u64,
    conflict_count: u64,
    primary_evidence_complete: bool,
    witness_evidence_complete: bool,
}

impl Default for PumpObservationLedgerV1 {
    fn default() -> Self {
        Self::with_validated_config(PumpObservationLedgerConfigV1::default())
    }
}

impl PumpObservationLedgerV1 {
    pub fn try_new(
        config: PumpObservationLedgerConfigV1,
    ) -> Result<Self, PumpObservationLedgerConfigErrorV1> {
        Ok(Self::with_validated_config(config.validate()?))
    }

    fn with_validated_config(config: PumpObservationLedgerConfigV1) -> Self {
        Self {
            config,
            canonical_by_locator: HashMap::new(),
            canonical_locators_by_signature: HashMap::new(),
            primary_transaction_inventories: HashMap::new(),
            pending_witnesses: Vec::new(),
            finalized_unassigned_witnesses: Vec::new(),
            expired_witness_audit: Vec::new(),
            expired_witness_audit_overflow_count: 0,
            first_rejected_expired_witness: None,
            retained_conflicts: Vec::new(),
            first_evidence_overflow: None,
            overflow_counts: HashMap::new(),
            canonical_mutation_count: 0,
            provider_observation_count: 0,
            exact_duplicate_count: 0,
            conflict_count: 0,
            primary_evidence_complete: true,
            witness_evidence_complete: true,
        }
    }

    /// Classify one observation synchronously.
    pub fn observe(
        &mut self,
        observation: ObservedPumpMutationV1,
        now_monotonic_ns: u64,
    ) -> PumpObservationLedgerResultV1 {
        self.provider_observation_count = self.provider_observation_count.saturating_add(1);

        match (
            observation.provenance.source_family,
            observation.raw_provider_role,
        ) {
            (
                ObservationSourceFamilyV1::RawYellowstone,
                Some(RawProviderRoleV1::PrimaryAuthority),
            ) => self.observe_primary_raw(observation, now_monotonic_ns),
            (
                ObservationSourceFamilyV1::RawYellowstone,
                Some(RawProviderRoleV1::SecondaryWitness),
            ) => self.observe_witness(observation, now_monotonic_ns, false),
            (ObservationSourceFamilyV1::ParsedNln, None) => {
                self.observe_witness(observation, now_monotonic_ns, true)
            }
            _ => self.invalid_primary_or_witness_contract(observation),
        }
    }

    /// Classify a primary raw observation whose transport wrapper disagrees
    /// with the normalized observation.
    ///
    /// This is the launcher IPC trust boundary: the ledger must account for
    /// the rejected provider observation, but it must not first insert a
    /// structural canonical mutation and only then discover that the
    /// independently serialized `CandidatePool`/`TradeEvent` payload differs.
    /// The complete observation remains available in the returned integrity
    /// signal hash and the primary segment becomes explicitly incomplete.
    #[must_use]
    pub fn observe_primary_boundary_mismatch(
        &mut self,
        observation: ObservedPumpMutationV1,
    ) -> PumpObservationLedgerResultV1 {
        self.provider_observation_count = self.provider_observation_count.saturating_add(1);
        self.primary_evidence_complete = false;
        let candidate = candidate_from_primary_claims(&observation);
        single_result(PumpObservationLedgerDecisionV1 {
            classification: PumpObservationClassificationV1::PrimaryRawCoverageIncomplete,
            correlation: None,
            provider_agreement: PumpProviderAgreementV1::NotObserved,
            conflict_fields: Vec::new(),
            canonical_mutation: None,
            candidate_integrity_signal: candidate.map(|candidate| {
                integrity_signal(
                    candidate,
                    CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                    &observation,
                    Vec::new(),
                )
            }),
            expired_witness_observation: None,
            evidence_complete: false,
        })
    }

    /// Finalize parsed witnesses whose bounded correlation window has elapsed.
    ///
    /// Raw primary canonical mutations are never delayed until this call.
    pub fn finalize_expired(
        &mut self,
        now_monotonic_ns: u64,
    ) -> Vec<PumpObservationLedgerDecisionV1> {
        let mut decisions = self.finalize_incomplete_primary_inventories(now_monotonic_ns);
        let mut expired_by_signature: HashMap<Signature, Vec<PendingWitness>> = HashMap::new();
        let mut retained = Vec::with_capacity(self.pending_witnesses.len());

        for pending in std::mem::take(&mut self.pending_witnesses) {
            let age = now_monotonic_ns.saturating_sub(pending.first_seen_monotonic_ns);
            if age >= self.config.correlation_window_ns
                && pending.observation.provenance.source_family
                    == ObservationSourceFamilyV1::RawYellowstone
                && pending.observation.raw_provider_role
                    == Some(RawProviderRoleV1::SecondaryWitness)
            {
                let expired_observation = pending.observation;
                let retained_expiry_evidence =
                    self.retain_expired_witness_audit(&expired_observation);
                decisions.push(PumpObservationLedgerDecisionV1 {
                    classification: PumpObservationClassificationV1::SecondaryWitnessExpired,
                    correlation: None,
                    provider_agreement: PumpProviderAgreementV1::WitnessOnly,
                    conflict_fields: Vec::new(),
                    canonical_mutation: None,
                    candidate_integrity_signal: None,
                    expired_witness_observation: Some(expired_observation),
                    evidence_complete: retained_expiry_evidence && self.witness_evidence_complete,
                });
            } else if pending.observation.provenance.source_family
                == ObservationSourceFamilyV1::ParsedNln
                && age >= self.config.correlation_window_ns
            {
                expired_by_signature
                    .entry(pending.observation.signature)
                    .or_default()
                    .push(pending);
            } else {
                retained.push(pending);
            }
        }
        self.pending_witnesses = retained;

        let mut signatures: Vec<_> = expired_by_signature.into_iter().collect();
        signatures.sort_by_key(|(signature, _)| signature.to_string());
        for (signature, mut parsed_witnesses) in signatures {
            // Once any locatorless witness for a signature expires, include
            // every already-observed locatorless parsed witness for that
            // signature in the same finalization decision. This prevents
            // staggered arrivals from each becoming a false singleton.
            if parsed_witnesses
                .iter()
                .any(|pending| pending.observation.locator_hint.is_none())
            {
                let mut still_pending = Vec::with_capacity(self.pending_witnesses.len());
                for pending in self.pending_witnesses.drain(..) {
                    if pending.observation.provenance.source_family
                        == ObservationSourceFamilyV1::ParsedNln
                        && pending.observation.signature == signature
                        && pending.observation.locator_hint.is_none()
                    {
                        parsed_witnesses.push(pending);
                    } else {
                        still_pending.push(pending);
                    }
                }
                self.pending_witnesses = still_pending;
            }

            let raw_locators = self
                .canonical_locators_by_signature
                .get(&signature)
                .cloned()
                .unwrap_or_default();
            let declared_singleton = raw_locators.len() == 1
                && self
                    .canonical_by_locator
                    .get(&raw_locators[0])
                    .and_then(|record| record.raw_transaction_mutation_count)
                    == Some(1);

            let mut locatorless = Vec::new();
            let mut nonmatching_complete = Vec::new();
            for pending in parsed_witnesses {
                if pending.observation.locator_hint.is_none() {
                    locatorless.push(pending);
                } else {
                    nonmatching_complete.push(pending);
                }
            }

            // A complete locator that did not match during observe/raw arrival
            // is independently unmatchable. It must not change the outcome of
            // locatorless witnesses sharing only the signature.
            for pending in nonmatching_complete {
                let retained = self.retain_finalized_unassigned(
                    pending,
                    ParsedWitnessCorrelationOutcomeV1::Unmatchable,
                );
                decisions.push(PumpObservationLedgerDecisionV1 {
                    classification: PumpObservationClassificationV1::UnmatchableParsedWitness,
                    correlation: Some(ParsedWitnessCorrelationOutcomeV1::Unmatchable),
                    provider_agreement: PumpProviderAgreementV1::WitnessOnly,
                    conflict_fields: Vec::new(),
                    canonical_mutation: None,
                    candidate_integrity_signal: None,
                    expired_witness_observation: None,
                    evidence_complete: retained && self.witness_evidence_complete,
                });
            }

            if declared_singleton && locatorless.len() == 1 {
                let pending = locatorless
                    .pop()
                    .expect("locatorless length was checked without concurrent mutation");
                decisions.push(self.correlate_with_canonical(
                    &raw_locators[0],
                    pending.observation,
                    Some(ParsedWitnessCorrelationOutcomeV1::UniqueSignatureSingletonMatch),
                ));
                continue;
            }

            let outcome = if raw_locators.is_empty() {
                ParsedWitnessCorrelationOutcomeV1::Unmatchable
            } else {
                ParsedWitnessCorrelationOutcomeV1::Ambiguous
            };
            for pending in locatorless {
                let retained = self.retain_finalized_unassigned(pending, outcome);
                decisions.push(PumpObservationLedgerDecisionV1 {
                    classification: match outcome {
                        ParsedWitnessCorrelationOutcomeV1::Ambiguous => {
                            PumpObservationClassificationV1::AmbiguousParsedWitness
                        }
                        ParsedWitnessCorrelationOutcomeV1::Unmatchable => {
                            PumpObservationClassificationV1::UnmatchableParsedWitness
                        }
                        _ => unreachable!(
                            "finalize branch constructs only terminal no-match outcomes"
                        ),
                    },
                    correlation: Some(outcome),
                    provider_agreement: PumpProviderAgreementV1::WitnessOnly,
                    conflict_fields: Vec::new(),
                    canonical_mutation: None,
                    candidate_integrity_signal: None,
                    expired_witness_observation: None,
                    evidence_complete: retained && self.witness_evidence_complete,
                });
            }
        }

        decisions
    }

    fn retain_finalized_unassigned(
        &mut self,
        pending: PendingWitness,
        outcome: ParsedWitnessCorrelationOutcomeV1,
    ) -> bool {
        let retained_witness_count = self
            .pending_witnesses
            .len()
            .saturating_add(self.finalized_unassigned_witnesses.len());
        if retained_witness_count >= self.config.max_pending_witnesses {
            self.witness_evidence_complete = false;
            self.note_overflow(
                PumpObservationEvidenceLaneV1::PendingWitness,
                &pending.observation,
                retained_witness_count,
            );
            return false;
        }
        self.finalized_unassigned_witnesses
            .push(FinalizedUnassignedWitness {
                observation: pending.observation,
                identity: pending.identity,
                outcome,
            });
        true
    }

    #[must_use]
    pub fn snapshot(&self) -> PumpObservationLedgerSnapshotV1 {
        PumpObservationLedgerSnapshotV1 {
            canonical_mutation_count: self.canonical_mutation_count,
            provider_observation_count: self.provider_observation_count,
            exact_duplicate_count: self.exact_duplicate_count,
            conflict_count: self.conflict_count,
            pending_witness_count: self.pending_witnesses.len(),
            retained_conflict_count: self.retained_conflicts.len(),
            finalized_unassigned_witness_count: self.finalized_unassigned_witnesses.len(),
            retained_expired_witness_count: self.expired_witness_audit.len(),
            expired_witness_audit_overflow_count: self.expired_witness_audit_overflow_count,
            evidence_overflow_count: self
                .overflow_counts
                .values()
                .copied()
                .fold(0_u64, u64::saturating_add),
            primary_evidence_complete: self.primary_evidence_complete,
            witness_evidence_complete: self.witness_evidence_complete,
            first_evidence_overflow: self.first_evidence_overflow.clone(),
            first_rejected_expired_witness: self.first_rejected_expired_witness.clone(),
        }
    }

    #[must_use]
    pub fn retained_conflicts(&self) -> &[PumpSourceConflictEvidenceV1] {
        &self.retained_conflicts
    }

    #[must_use]
    pub fn retained_expired_witnesses(&self) -> &[ObservedPumpMutationV1] {
        &self.expired_witness_audit
    }

    fn observe_primary_raw(
        &mut self,
        observation: ObservedPumpMutationV1,
        now_monotonic_ns: u64,
    ) -> PumpObservationLedgerResultV1 {
        let candidate = candidate_from_primary_claims(&observation);
        if let Some(reason) = validate_primary_observation(&observation) {
            self.primary_evidence_complete = false;
            let signal = candidate.map(|candidate| {
                integrity_signal(
                    candidate,
                    CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                    &observation,
                    Vec::new(),
                )
            });
            return single_result(PumpObservationLedgerDecisionV1 {
                classification: reason,
                correlation: None,
                provider_agreement: PumpProviderAgreementV1::NotObserved,
                conflict_fields: Vec::new(),
                canonical_mutation: None,
                candidate_integrity_signal: signal,
                expired_witness_observation: None,
                evidence_complete: false,
            });
        }

        let locator = observation
            .locator_hint
            .clone()
            .expect("validated primary locator");
        if !self.primary_transaction_inventory_is_consistent(&observation, &locator) {
            self.primary_evidence_complete = false;
            return single_result(PumpObservationLedgerDecisionV1 {
                classification: PumpObservationClassificationV1::PrimaryRawCoverageIncomplete,
                correlation: None,
                provider_agreement: PumpProviderAgreementV1::NotObserved,
                conflict_fields: Vec::new(),
                canonical_mutation: None,
                candidate_integrity_signal: candidate.map(|candidate| {
                    integrity_signal(
                        candidate,
                        CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                        &observation,
                        Vec::new(),
                    )
                }),
                expired_witness_observation: None,
                evidence_complete: false,
            });
        }
        if self.canonical_by_locator.contains_key(&locator) {
            let decision = self.correlate_with_canonical(&locator, observation, None);
            return single_result(decision);
        }

        if self.canonical_by_locator.len() >= self.config.max_primary_canonical_mutations {
            self.primary_evidence_complete = false;
            self.note_overflow(
                PumpObservationEvidenceLaneV1::PrimaryCanonical,
                &observation,
                self.canonical_by_locator.len(),
            );
            return single_result(PumpObservationLedgerDecisionV1 {
                classification: PumpObservationClassificationV1::PrimaryRawCoverageIncomplete,
                correlation: None,
                provider_agreement: PumpProviderAgreementV1::NotObserved,
                conflict_fields: Vec::new(),
                canonical_mutation: None,
                candidate_integrity_signal: candidate.map(|candidate| {
                    integrity_signal(
                        candidate,
                        CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                        &observation,
                        Vec::new(),
                    )
                }),
                expired_witness_observation: None,
                evidence_complete: false,
            });
        }

        let canonical = StructuralCanonicalPumpMutationV1 {
            mutation_family: observation.mutation_family,
            locator: locator.clone(),
            order: observation
                .canonical_order
                .clone()
                .expect("validated primary canonical order"),
            claims: observation.claims.clone(),
            primary_raw_provenance: observation.provenance.clone(),
            economics_status: PumpEconomicCertificationStatusV1::PendingAnchor,
        };
        let mut identities = HashSet::new();
        identities.insert(ProviderObservationIdentity::from(&observation));
        self.canonical_by_locator.insert(
            locator.clone(),
            CanonicalRecord {
                canonical: canonical.clone(),
                candidate,
                primary_observation: observation.clone(),
                raw_transaction_mutation_count: observation.raw_transaction_mutation_count,
                observation_identities: identities,
                correlated_witnesses: Vec::new(),
            },
        );
        let signature_locators = self
            .canonical_locators_by_signature
            .entry(observation.signature)
            .or_default();
        if !signature_locators.contains(&locator) {
            signature_locators.push(locator.clone());
        }
        self.canonical_mutation_count = self.canonical_mutation_count.saturating_add(1);

        let inventory_complete_now =
            if let Some(declared_count) = observation.raw_transaction_mutation_count {
                let inventory = self
                    .primary_transaction_inventories
                    .entry(observation.signature)
                    .or_insert(PrimaryTransactionInventory {
                        declared_count,
                        first_seen_monotonic_ns: now_monotonic_ns,
                        completion_signaled: false,
                        incomplete_signaled: false,
                    });
                inventory.first_seen_monotonic_ns =
                    inventory.first_seen_monotonic_ns.min(now_monotonic_ns);

                let complete = self
                    .canonical_locators_by_signature
                    .get(&observation.signature)
                    .is_some_and(|locators| {
                        u32::try_from(locators.len()).ok() == Some(inventory.declared_count)
                    })
                    && !inventory.completion_signaled;
                if complete {
                    inventory.completion_signaled = true;
                }
                complete
            } else {
                self.primary_evidence_complete = false;
                false
            };

        let mut primary_decision = PumpObservationLedgerDecisionV1 {
            classification: PumpObservationClassificationV1::PrimaryCanonicalApplied,
            correlation: None,
            provider_agreement: PumpProviderAgreementV1::NotObserved,
            conflict_fields: Vec::new(),
            canonical_mutation: Some(canonical),
            candidate_integrity_signal: observation
                .raw_transaction_mutation_count
                .is_none()
                .then(|| {
                    candidate.map(|candidate| {
                        integrity_signal(
                            candidate,
                            CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                            &observation,
                            Vec::new(),
                        )
                    })
                })
                .flatten(),
            expired_witness_observation: None,
            evidence_complete: self.primary_evidence_complete
                && observation.raw_transaction_mutation_count.is_some(),
        };

        let mut matching = Vec::new();
        let mut retained = Vec::with_capacity(self.pending_witnesses.len());
        for pending in self.pending_witnesses.drain(..) {
            if pending.observation.locator_hint.as_ref() == Some(&locator) {
                matching.push(pending.observation);
            } else {
                retained.push(pending);
            }
        }
        self.pending_witnesses = retained;

        // A complete witness can have finalized as unmatchable before its raw
        // transaction arrived. Preserve append-only history, but now attach
        // that retained evidence to the exact structural locator so both
        // arrival orders converge and subsequent replays are duplicates.
        let mut finalized_retained = Vec::with_capacity(self.finalized_unassigned_witnesses.len());
        for finalized in self.finalized_unassigned_witnesses.drain(..) {
            if finalized.observation.locator_hint.as_ref() == Some(&locator) {
                matching.push(finalized.observation);
            } else {
                finalized_retained.push(finalized);
            }
        }
        self.finalized_unassigned_witnesses = finalized_retained;

        let mut derived_decisions: Vec<_> = matching
            .into_iter()
            .map(|witness| {
                let correlation = (witness.provenance.source_family
                    == ObservationSourceFamilyV1::ParsedNln)
                    .then_some(ParsedWitnessCorrelationOutcomeV1::ExactStructuralMatch);
                self.correlate_with_canonical(&locator, witness, correlation)
            })
            .collect();

        if inventory_complete_now {
            let mut ready_candidates = HashSet::new();
            if let Some(locators) = self
                .canonical_locators_by_signature
                .get(&observation.signature)
            {
                for inventory_locator in locators {
                    let Some(record) = self.canonical_by_locator.get(inventory_locator) else {
                        continue;
                    };
                    let Some(candidate) = record.candidate else {
                        continue;
                    };
                    if !ready_candidates.insert(candidate) {
                        continue;
                    }
                    let signal = integrity_signal(
                        candidate,
                        CandidateIntegrityOutcomeV1::Ready,
                        &record.primary_observation,
                        Vec::new(),
                    );
                    if candidate == candidate_from_primary_claims(&observation).unwrap_or(candidate)
                    {
                        primary_decision.candidate_integrity_signal = Some(signal);
                    } else {
                        derived_decisions.push(PumpObservationLedgerDecisionV1 {
                            classification:
                                PumpObservationClassificationV1::PrimaryTransactionInventoryComplete,
                            correlation: None,
                            provider_agreement: PumpProviderAgreementV1::NotObserved,
                            conflict_fields: Vec::new(),
                            canonical_mutation: None,
                            candidate_integrity_signal: Some(signal),
                            expired_witness_observation: None,
                            evidence_complete: self.primary_evidence_complete,
                        });
                    }
                }
            }
        }

        let signature_has_one_canonical_locator = self
            .canonical_locators_by_signature
            .get(&observation.signature)
            .is_some_and(|locators| locators.len() == 1);
        if observation.raw_transaction_mutation_count == Some(1)
            && signature_has_one_canonical_locator
        {
            let locatorless_matches: Vec<_> = self
                .finalized_unassigned_witnesses
                .iter()
                .enumerate()
                .filter_map(|(index, finalized)| {
                    (finalized.observation.provenance.source_family
                        == ObservationSourceFamilyV1::ParsedNln
                        && finalized.observation.signature == observation.signature
                        && finalized.observation.locator_hint.is_none())
                    .then_some(index)
                })
                .collect();
            if let [index] = locatorless_matches.as_slice() {
                let finalized = self.finalized_unassigned_witnesses.remove(*index);
                derived_decisions.push(self.correlate_with_canonical(
                    &locator,
                    finalized.observation,
                    Some(ParsedWitnessCorrelationOutcomeV1::UniqueSignatureSingletonMatch),
                ));
            }
        }

        PumpObservationLedgerResultV1 {
            observation_decision: primary_decision,
            derived_decisions,
        }
    }

    fn finalize_incomplete_primary_inventories(
        &mut self,
        now_monotonic_ns: u64,
    ) -> Vec<PumpObservationLedgerDecisionV1> {
        let expired: Vec<_> = self
            .primary_transaction_inventories
            .iter_mut()
            .filter_map(|(signature, inventory)| {
                let observed_count = self
                    .canonical_locators_by_signature
                    .get(signature)
                    .map_or(0, Vec::len);
                let incomplete = u32::try_from(observed_count)
                    .map_or(true, |observed| observed < inventory.declared_count);
                let expired = now_monotonic_ns.saturating_sub(inventory.first_seen_monotonic_ns)
                    >= self.config.correlation_window_ns;
                if incomplete && expired && !inventory.incomplete_signaled {
                    inventory.incomplete_signaled = true;
                    Some(*signature)
                } else {
                    None
                }
            })
            .collect();

        let mut decisions = Vec::new();
        for signature in expired {
            self.primary_evidence_complete = false;
            let mut candidates = HashSet::new();
            if let Some(locators) = self.canonical_locators_by_signature.get(&signature) {
                for locator in locators {
                    let Some(record) = self.canonical_by_locator.get(locator) else {
                        continue;
                    };
                    let Some(candidate) = record.candidate else {
                        continue;
                    };
                    if !candidates.insert(candidate) {
                        continue;
                    }
                    decisions.push(PumpObservationLedgerDecisionV1 {
                        classification:
                            PumpObservationClassificationV1::PrimaryRawCoverageIncomplete,
                        correlation: None,
                        provider_agreement: PumpProviderAgreementV1::NotObserved,
                        conflict_fields: Vec::new(),
                        canonical_mutation: None,
                        candidate_integrity_signal: Some(integrity_signal(
                            candidate,
                            CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                            &record.primary_observation,
                            Vec::new(),
                        )),
                        expired_witness_observation: None,
                        evidence_complete: false,
                    });
                }
            }
        }
        decisions
    }

    fn primary_transaction_inventory_is_consistent(
        &self,
        observation: &ObservedPumpMutationV1,
        locator: &RawPumpMutationLocatorV1,
    ) -> bool {
        let Some(declared_count) = observation.raw_transaction_mutation_count else {
            return true;
        };
        if self
            .primary_transaction_inventories
            .get(&observation.signature)
            .is_some_and(|inventory| inventory.declared_count != declared_count)
        {
            return false;
        }
        let Some(existing_locators) = self
            .canonical_locators_by_signature
            .get(&observation.signature)
        else {
            return true;
        };

        if existing_locators.iter().any(|existing_locator| {
            self.canonical_by_locator
                .get(existing_locator)
                .and_then(|record| record.raw_transaction_mutation_count)
                .is_some_and(|existing_count| existing_count != declared_count)
        }) {
            return false;
        }

        existing_locators.contains(locator)
            || existing_locators.len() < usize::try_from(declared_count).unwrap_or(usize::MAX)
    }

    fn observe_witness(
        &mut self,
        observation: ObservedPumpMutationV1,
        now_monotonic_ns: u64,
        parsed_nln: bool,
    ) -> PumpObservationLedgerResultV1 {
        if !valid_common_provenance(&observation)
            || observation
                .locator_hint
                .as_ref()
                .is_some_and(|locator| locator.signature != observation.signature)
        {
            return single_result(PumpObservationLedgerDecisionV1 {
                classification: PumpObservationClassificationV1::EvidenceCapacityExceeded,
                correlation: None,
                provider_agreement: PumpProviderAgreementV1::WitnessOnly,
                conflict_fields: Vec::new(),
                canonical_mutation: None,
                candidate_integrity_signal: None,
                expired_witness_observation: None,
                evidence_complete: false,
            });
        }

        if let Some(locator) = observation.locator_hint.clone() {
            if self.canonical_by_locator.contains_key(&locator) {
                let correlation =
                    parsed_nln.then_some(ParsedWitnessCorrelationOutcomeV1::ExactStructuralMatch);
                return single_result(self.correlate_with_canonical(
                    &locator,
                    observation,
                    correlation,
                ));
            }
        }

        let identity = ProviderObservationIdentity::from(&observation);
        if let Some(finalized_outcome) =
            self.finalized_unassigned_witnesses
                .iter()
                .find_map(|finalized| {
                    (finalized.identity == identity
                        && same_normalized_observation(&finalized.observation, &observation))
                    .then_some(finalized.outcome)
                })
        {
            self.exact_duplicate_count = self.exact_duplicate_count.saturating_add(1);
            return single_result(PumpObservationLedgerDecisionV1 {
                classification: PumpObservationClassificationV1::ExactDuplicate,
                correlation: Some(finalized_outcome),
                provider_agreement: PumpProviderAgreementV1::WitnessOnly,
                conflict_fields: Vec::new(),
                canonical_mutation: None,
                candidate_integrity_signal: None,
                expired_witness_observation: None,
                evidence_complete: self.witness_evidence_complete,
            });
        }
        let pending_exact_normalized_observation = self.pending_witnesses.iter().any(|pending| {
            pending.identity == identity
                && same_normalized_observation(&pending.observation, &observation)
        });
        if pending_exact_normalized_observation {
            self.exact_duplicate_count = self.exact_duplicate_count.saturating_add(1);
            return single_result(PumpObservationLedgerDecisionV1 {
                classification: PumpObservationClassificationV1::ExactDuplicate,
                correlation: None,
                provider_agreement: PumpProviderAgreementV1::WitnessOnly,
                conflict_fields: Vec::new(),
                canonical_mutation: None,
                candidate_integrity_signal: None,
                expired_witness_observation: None,
                evidence_complete: self.witness_evidence_complete,
            });
        }

        let retained_witness_count = self
            .pending_witnesses
            .len()
            .saturating_add(self.finalized_unassigned_witnesses.len());
        if retained_witness_count >= self.config.max_pending_witnesses {
            self.witness_evidence_complete = false;
            self.note_overflow(
                PumpObservationEvidenceLaneV1::PendingWitness,
                &observation,
                retained_witness_count,
            );
            return single_result(PumpObservationLedgerDecisionV1 {
                classification: PumpObservationClassificationV1::EvidenceCapacityExceeded,
                correlation: None,
                provider_agreement: PumpProviderAgreementV1::WitnessOnly,
                conflict_fields: Vec::new(),
                canonical_mutation: None,
                candidate_integrity_signal: None,
                expired_witness_observation: None,
                evidence_complete: false,
            });
        }

        self.pending_witnesses.push(PendingWitness {
            observation,
            identity,
            first_seen_monotonic_ns: now_monotonic_ns,
        });
        single_result(PumpObservationLedgerDecisionV1 {
            classification: if parsed_nln {
                PumpObservationClassificationV1::ParsedWitnessPending
            } else {
                PumpObservationClassificationV1::SecondaryWitnessOnly
            },
            correlation: None,
            provider_agreement: PumpProviderAgreementV1::WitnessOnly,
            conflict_fields: Vec::new(),
            canonical_mutation: None,
            candidate_integrity_signal: None,
            expired_witness_observation: None,
            evidence_complete: self.witness_evidence_complete,
        })
    }

    fn correlate_with_canonical(
        &mut self,
        locator: &RawPumpMutationLocatorV1,
        observation: ObservedPumpMutationV1,
        correlation: Option<ParsedWitnessCorrelationOutcomeV1>,
    ) -> PumpObservationLedgerDecisionV1 {
        let identity = ProviderObservationIdentity::from(&observation);
        let incoming_is_primary = observation.provenance.source_family
            == ObservationSourceFamilyV1::RawYellowstone
            && observation.raw_provider_role == Some(RawProviderRoleV1::PrimaryAuthority);
        let (
            exact_identity,
            exact_normalized_observation,
            canonical,
            candidate,
            primary_provenance,
            correlated_witness_count,
        ) = {
            let record = self
                .canonical_by_locator
                .get(locator)
                .expect("caller checked canonical locator");
            (
                record.observation_identities.contains(&identity),
                record.correlated_witnesses.iter().any(|retained| {
                    ProviderObservationIdentity::from(retained) == identity
                        && same_normalized_observation(retained, &observation)
                }),
                record.canonical.clone(),
                record.candidate,
                record.canonical.primary_raw_provenance.clone(),
                record.correlated_witnesses.len(),
            )
        };
        let conflict_fields = material_conflict_fields(&canonical, &observation);

        if exact_identity {
            // A captured provider payload cannot legitimately normalize into
            // two different structural facts. The first such contradiction is
            // retained as a typed source conflict; only an exact replay of that
            // already-retained contradictory normalization is a duplicate.
            if !conflict_fields.is_empty() && !exact_normalized_observation {
                let retained_observation = self.retain_correlated_observation(
                    locator,
                    identity,
                    &observation,
                    correlated_witness_count,
                );
                return self.source_conflict_decision(
                    locator,
                    primary_provenance,
                    candidate,
                    observation,
                    correlation,
                    conflict_fields,
                    incoming_is_primary,
                    retained_observation,
                );
            }
            self.exact_duplicate_count = self.exact_duplicate_count.saturating_add(1);
            let provider_agreement = if !conflict_fields.is_empty() {
                if incoming_is_primary {
                    PumpProviderAgreementV1::NotObserved
                } else {
                    PumpProviderAgreementV1::PrimarySecondaryConflict
                }
            } else if incoming_is_primary {
                PumpProviderAgreementV1::NotObserved
            } else {
                PumpProviderAgreementV1::PrimarySecondaryAgreement
            };
            return PumpObservationLedgerDecisionV1 {
                classification: PumpObservationClassificationV1::ExactDuplicate,
                correlation,
                provider_agreement,
                conflict_fields,
                canonical_mutation: None,
                candidate_integrity_signal: None,
                expired_witness_observation: None,
                evidence_complete: self.witness_evidence_complete,
            };
        }

        let is_conflict = !conflict_fields.is_empty();
        let retained_observation = self.retain_correlated_observation(
            locator,
            identity,
            &observation,
            correlated_witness_count,
        );

        if is_conflict {
            return self.source_conflict_decision(
                locator,
                primary_provenance,
                candidate,
                observation,
                correlation,
                conflict_fields,
                incoming_is_primary,
                retained_observation,
            );
        }

        PumpObservationLedgerDecisionV1 {
            classification: match correlation {
                Some(ParsedWitnessCorrelationOutcomeV1::ExactStructuralMatch) => {
                    PumpObservationClassificationV1::ExactStructuralMatch
                }
                Some(ParsedWitnessCorrelationOutcomeV1::UniqueSignatureSingletonMatch) => {
                    PumpObservationClassificationV1::UniqueSignatureSingletonMatch
                }
                _ => PumpObservationClassificationV1::SameMutationAgreement,
            },
            correlation,
            provider_agreement: if incoming_is_primary {
                PumpProviderAgreementV1::NotObserved
            } else {
                PumpProviderAgreementV1::PrimarySecondaryAgreement
            },
            conflict_fields: Vec::new(),
            canonical_mutation: None,
            candidate_integrity_signal: None,
            expired_witness_observation: None,
            evidence_complete: retained_observation && self.witness_evidence_complete,
        }
    }

    fn retain_correlated_observation(
        &mut self,
        locator: &RawPumpMutationLocatorV1,
        identity: ProviderObservationIdentity,
        observation: &ObservedPumpMutationV1,
        correlated_witness_count: usize,
    ) -> bool {
        if correlated_witness_count < self.config.max_correlated_witnesses_per_mutation {
            if let Some(record) = self.canonical_by_locator.get_mut(locator) {
                record.observation_identities.insert(identity);
                record.correlated_witnesses.push(observation.clone());
                return true;
            }
        } else {
            self.witness_evidence_complete = false;
            self.note_overflow(
                PumpObservationEvidenceLaneV1::CorrelatedWitness,
                observation,
                correlated_witness_count,
            );
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn source_conflict_decision(
        &mut self,
        locator: &RawPumpMutationLocatorV1,
        primary_provenance: ObservationProvenanceV1,
        candidate: Option<PumpCandidateIdentityV1>,
        observation: ObservedPumpMutationV1,
        correlation: Option<ParsedWitnessCorrelationOutcomeV1>,
        conflict_fields: Vec<PumpMutationConflictFieldV1>,
        incoming_is_primary: bool,
        retained_observation: bool,
    ) -> PumpObservationLedgerDecisionV1 {
        self.conflict_count = self.conflict_count.saturating_add(1);
        let conflict = PumpSourceConflictEvidenceV1 {
            locator: locator.clone(),
            primary_raw_provenance: primary_provenance,
            witness_provenance: observation.provenance.clone(),
            conflict_fields: conflict_fields.clone(),
            correlation,
        };
        if self.retained_conflicts.len() < self.config.max_retained_conflicts {
            self.retained_conflicts.push(conflict);
        } else {
            self.witness_evidence_complete = false;
            self.note_overflow(
                PumpObservationEvidenceLaneV1::Conflict,
                &observation,
                self.retained_conflicts.len(),
            );
        }
        PumpObservationLedgerDecisionV1 {
            classification: PumpObservationClassificationV1::SourceReconciliationConflict,
            correlation,
            provider_agreement: if incoming_is_primary {
                PumpProviderAgreementV1::NotObserved
            } else {
                PumpProviderAgreementV1::PrimarySecondaryConflict
            },
            conflict_fields: conflict_fields.clone(),
            canonical_mutation: None,
            candidate_integrity_signal: candidate.map(|candidate| {
                integrity_signal(
                    candidate,
                    CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
                    &observation,
                    conflict_fields,
                )
            }),
            expired_witness_observation: None,
            evidence_complete: retained_observation && self.witness_evidence_complete,
        }
    }

    fn invalid_primary_or_witness_contract(
        &mut self,
        observation: ObservedPumpMutationV1,
    ) -> PumpObservationLedgerResultV1 {
        let declared_primary =
            observation.raw_provider_role == Some(RawProviderRoleV1::PrimaryAuthority);
        if declared_primary {
            self.primary_evidence_complete = false;
        } else {
            self.witness_evidence_complete = false;
        }
        let candidate = candidate_from_primary_claims(&observation);
        single_result(PumpObservationLedgerDecisionV1 {
            classification: if declared_primary {
                PumpObservationClassificationV1::PrimaryRawCoverageIncomplete
            } else {
                PumpObservationClassificationV1::EvidenceCapacityExceeded
            },
            correlation: None,
            provider_agreement: PumpProviderAgreementV1::NotObserved,
            conflict_fields: Vec::new(),
            canonical_mutation: None,
            candidate_integrity_signal: candidate.map(|candidate| {
                integrity_signal(
                    candidate,
                    CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                    &observation,
                    Vec::new(),
                )
            }),
            expired_witness_observation: None,
            evidence_complete: false,
        })
    }

    fn retain_expired_witness_audit(&mut self, observation: &ObservedPumpMutationV1) -> bool {
        if self.expired_witness_audit.len() < self.config.max_pending_witnesses {
            self.expired_witness_audit.push(observation.clone());
            return true;
        }

        self.witness_evidence_complete = false;
        self.expired_witness_audit_overflow_count =
            self.expired_witness_audit_overflow_count.saturating_add(1);
        if self.first_rejected_expired_witness.is_none() {
            self.first_rejected_expired_witness = Some(observation.clone());
        }
        self.note_overflow(
            PumpObservationEvidenceLaneV1::ExpiredWitnessAudit,
            observation,
            self.expired_witness_audit.len(),
        );
        false
    }

    fn note_overflow(
        &mut self,
        lane: PumpObservationEvidenceLaneV1,
        observation: &ObservedPumpMutationV1,
        retained_count: usize,
    ) {
        let count = self.overflow_counts.entry(lane).or_insert(0);
        *count = count.saturating_add(1);
        if self.first_evidence_overflow.is_none() {
            self.first_evidence_overflow = Some(PumpObservationEvidenceOverflowV1 {
                lane,
                first_rejected_observation: observation.clone(),
                retained_count,
                overflow_count: *count,
            });
        }
    }
}

fn single_result(
    observation_decision: PumpObservationLedgerDecisionV1,
) -> PumpObservationLedgerResultV1 {
    PumpObservationLedgerResultV1 {
        observation_decision,
        derived_decisions: Vec::new(),
    }
}

fn valid_common_provenance(observation: &ObservedPumpMutationV1) -> bool {
    !observation.provenance.source_id.trim().is_empty()
        && !observation.provenance.provider_id.trim().is_empty()
        && !observation.provenance.schema_id.trim().is_empty()
        && observation.provenance.payload_hash_blake3 != [0; 32]
}

fn validate_primary_observation(
    observation: &ObservedPumpMutationV1,
) -> Option<PumpObservationClassificationV1> {
    if !valid_common_provenance(observation)
        || observation.provenance.source_family != ObservationSourceFamilyV1::RawYellowstone
        || observation.raw_provider_role != Some(RawProviderRoleV1::PrimaryAuthority)
        || observation.locator_hint.is_none()
        || observation.canonical_order.is_none()
    {
        return Some(PumpObservationClassificationV1::PrimaryRawCoverageIncomplete);
    }
    let locator = observation.locator_hint.as_ref()?;
    let order = observation.canonical_order.as_ref()?;
    if locator.signature != observation.signature
        || locator.outer_instruction_index != order.outer_instruction_index
        || locator.inner_instruction_path != order.inner_instruction_path
        || locator.semantic_event_ordinal != order.semantic_event_ordinal
        || observation.raw_transaction_mutation_count == Some(0)
    {
        return Some(PumpObservationClassificationV1::PrimaryRawCoverageIncomplete);
    }
    None
}

fn candidate_from_primary_claims(
    observation: &ObservedPumpMutationV1,
) -> Option<PumpCandidateIdentityV1> {
    Some(PumpCandidateIdentityV1 {
        pool_amm_id: observation.claims.curve?,
        mint: observation.claims.mint?,
    })
}

fn same_normalized_observation(
    left: &ObservedPumpMutationV1,
    right: &ObservedPumpMutationV1,
) -> bool {
    left.mutation_family == right.mutation_family
        && left.signature == right.signature
        && left.locator_hint == right.locator_hint
        && left.canonical_order == right.canonical_order
        && left.raw_transaction_mutation_count == right.raw_transaction_mutation_count
        && left.claims == right.claims
        && left.raw_provider_role == right.raw_provider_role
}

fn material_conflict_fields(
    canonical: &StructuralCanonicalPumpMutationV1,
    witness: &ObservedPumpMutationV1,
) -> Vec<PumpMutationConflictFieldV1> {
    let mut fields = Vec::new();
    if canonical.mutation_family != witness.mutation_family {
        fields.push(PumpMutationConflictFieldV1::MutationFamily);
    }
    push_concrete_conflict(
        &mut fields,
        witness.canonical_order.as_ref(),
        Some(&canonical.order),
        PumpMutationConflictFieldV1::CanonicalOrder,
    );
    push_concrete_conflict(
        &mut fields,
        witness.claims.curve.as_ref(),
        canonical.claims.curve.as_ref(),
        PumpMutationConflictFieldV1::Curve,
    );
    push_concrete_conflict(
        &mut fields,
        witness.claims.mint.as_ref(),
        canonical.claims.mint.as_ref(),
        PumpMutationConflictFieldV1::Mint,
    );
    push_concrete_conflict(
        &mut fields,
        witness.claims.route_variant.as_ref(),
        canonical.claims.route_variant.as_ref(),
        PumpMutationConflictFieldV1::RouteVariant,
    );
    push_concrete_conflict(
        &mut fields,
        witness.claims.side.as_ref(),
        canonical.claims.side.as_ref(),
        PumpMutationConflictFieldV1::Side,
    );
    push_concrete_conflict(
        &mut fields,
        witness.claims.success.as_ref(),
        canonical.claims.success.as_ref(),
        PumpMutationConflictFieldV1::Success,
    );
    push_concrete_conflict(
        &mut fields,
        witness.claims.error_code.as_ref(),
        canonical.claims.error_code.as_ref(),
        PumpMutationConflictFieldV1::ErrorCode,
    );
    push_concrete_conflict(
        &mut fields,
        witness.claims.token_amount_units.as_ref(),
        canonical.claims.token_amount_units.as_ref(),
        PumpMutationConflictFieldV1::TokenAmountUnits,
    );
    push_concrete_conflict(
        &mut fields,
        witness.claims.instruction_limit.as_ref(),
        canonical.claims.instruction_limit.as_ref(),
        PumpMutationConflictFieldV1::InstructionLimit,
    );
    push_concrete_conflict(
        &mut fields,
        witness.claims.reported_curve_quote_lamports.as_ref(),
        canonical.claims.reported_curve_quote_lamports.as_ref(),
        PumpMutationConflictFieldV1::ReportedCurveQuoteLamports,
    );
    push_concrete_conflict(
        &mut fields,
        witness.claims.reported_wallet_delta_lamports.as_ref(),
        canonical.claims.reported_wallet_delta_lamports.as_ref(),
        PumpMutationConflictFieldV1::ReportedWalletDeltaLamports,
    );
    push_concrete_conflict(
        &mut fields,
        witness.claims.reported_fee_breakdown.as_ref(),
        canonical.claims.reported_fee_breakdown.as_ref(),
        PumpMutationConflictFieldV1::ReportedFeeBreakdown,
    );
    push_concrete_conflict(
        &mut fields,
        witness.claims.reported_post_state_hash_blake3.as_ref(),
        canonical.claims.reported_post_state_hash_blake3.as_ref(),
        PumpMutationConflictFieldV1::ReportedPostStateHashBlake3,
    );
    fields
}

fn push_concrete_conflict<T: PartialEq>(
    fields: &mut Vec<PumpMutationConflictFieldV1>,
    witness: Option<&T>,
    primary: Option<&T>,
    field: PumpMutationConflictFieldV1,
) {
    if matches!((witness, primary), (Some(left), Some(right)) if left != right) {
        fields.push(field);
    }
}

fn integrity_signal(
    candidate: PumpCandidateIdentityV1,
    outcome: CandidateIntegrityOutcomeV1,
    observation: &ObservedPumpMutationV1,
    conflict_fields: Vec<PumpMutationConflictFieldV1>,
) -> CandidateIntegritySignalV1 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"ghost.candidate_integrity_signal.v1");
    hasher.update(candidate.pool_amm_id.as_ref());
    hasher.update(candidate.mint.as_ref());
    hasher.update(outcome_tag(outcome));
    hasher.update(observation.signature.as_ref());
    if let Some(locator) = observation.locator_hint.as_ref() {
        hasher.update(locator.program_id.as_ref());
        hasher.update(&locator.outer_instruction_index.to_le_bytes());
        for component in &locator.inner_instruction_path {
            hasher.update(&component.to_le_bytes());
        }
        hasher.update(&locator.semantic_event_ordinal.to_le_bytes());
    }
    for field in &conflict_fields {
        hasher.update(conflict_field_tag(*field));
    }
    hasher.update(&observation.provenance.payload_hash_blake3);

    CandidateIntegritySignalV1 {
        candidate,
        outcome,
        signature: Some(observation.signature),
        locator: observation.locator_hint.clone(),
        conflict_fields,
        evidence_hash_blake3: *hasher.finalize().as_bytes(),
    }
}

fn outcome_tag(outcome: CandidateIntegrityOutcomeV1) -> &'static [u8] {
    match outcome {
        CandidateIntegrityOutcomeV1::Ready => b"ready",
        CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete => {
            b"primary_raw_coverage_incomplete"
        }
        CandidateIntegrityOutcomeV1::AccountProviderConflict => b"account_provider_conflict",
        CandidateIntegrityOutcomeV1::SourceReconciliationConflict => {
            b"source_reconciliation_conflict"
        }
        CandidateIntegrityOutcomeV1::AnchorMissing => b"anchor_missing",
        CandidateIntegrityOutcomeV1::EconomicsNonEvaluable => b"economics_non_evaluable",
    }
}

fn conflict_field_tag(field: PumpMutationConflictFieldV1) -> &'static [u8] {
    match field {
        PumpMutationConflictFieldV1::MutationFamily => b"mutation_family",
        PumpMutationConflictFieldV1::CanonicalOrder => b"canonical_order",
        PumpMutationConflictFieldV1::Curve => b"curve",
        PumpMutationConflictFieldV1::Mint => b"mint",
        PumpMutationConflictFieldV1::RouteVariant => b"route_variant",
        PumpMutationConflictFieldV1::Side => b"side",
        PumpMutationConflictFieldV1::Success => b"success",
        PumpMutationConflictFieldV1::ErrorCode => b"error_code",
        PumpMutationConflictFieldV1::TokenAmountUnits => b"token_amount_units",
        PumpMutationConflictFieldV1::InstructionLimit => b"instruction_limit",
        PumpMutationConflictFieldV1::ReportedCurveQuoteLamports => b"reported_curve_quote_lamports",
        PumpMutationConflictFieldV1::ReportedWalletDeltaLamports => {
            b"reported_wallet_delta_lamports"
        }
        PumpMutationConflictFieldV1::ReportedFeeBreakdown => b"reported_fee_breakdown",
        PumpMutationConflictFieldV1::ReportedPostStateHashBlake3 => {
            b"reported_post_state_hash_blake3"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest_integrity::{
        CanonicalPumpOrderKeyV1, ObservationProvenanceV1, PumpMutationClaimsV1,
        PumpMutationFamilyV1, PumpTradeSideV1,
    };
    use solana_sdk::pubkey::Pubkey;

    fn locator(signature: Signature, ordinal: u32) -> RawPumpMutationLocatorV1 {
        RawPumpMutationLocatorV1 {
            program_id: Pubkey::new_unique(),
            signature,
            outer_instruction_index: 1,
            inner_instruction_path: vec![0],
            semantic_event_ordinal: ordinal,
        }
    }

    fn claims(curve: Pubkey, mint: Pubkey, amount: u64) -> PumpMutationClaimsV1 {
        PumpMutationClaimsV1 {
            curve: Some(curve),
            mint: Some(mint),
            side: Some(PumpTradeSideV1::Buy),
            success: Some(true),
            token_amount_units: Some(amount),
            ..PumpMutationClaimsV1::default()
        }
    }

    fn provenance(
        family: ObservationSourceFamilyV1,
        provider: &str,
        hash_byte: u8,
        received_at: u64,
    ) -> ObservationProvenanceV1 {
        ObservationProvenanceV1 {
            source_family: family,
            source_id: match family {
                ObservationSourceFamilyV1::RawYellowstone => "grpc_global_stream",
                ObservationSourceFamilyV1::ParsedNln => "nln_pumpfun_trade",
            }
            .to_owned(),
            provider_id: provider.to_owned(),
            schema_id: match family {
                ObservationSourceFamilyV1::RawYellowstone => {
                    "yellowstone_subscribe_update_transaction.prost.v1"
                }
                ObservationSourceFamilyV1::ParsedNln => "nln_program_stream_payload_json.v1",
            }
            .to_owned(),
            payload_hash_blake3: [hash_byte; 32],
            received_at_monotonic_ns: received_at,
        }
    }

    fn raw(
        locator: RawPumpMutationLocatorV1,
        role: RawProviderRoleV1,
        provider: &str,
        hash_byte: u8,
        curve: Pubkey,
        mint: Pubkey,
        amount: u64,
        declared_count: u32,
    ) -> ObservedPumpMutationV1 {
        ObservedPumpMutationV1 {
            mutation_family: PumpMutationFamilyV1::Trade,
            signature: locator.signature,
            canonical_order: Some(CanonicalPumpOrderKeyV1 {
                slot: 11,
                tx_index: 0,
                outer_instruction_index: locator.outer_instruction_index,
                inner_instruction_path: locator.inner_instruction_path.clone(),
                semantic_event_ordinal: locator.semantic_event_ordinal,
            }),
            locator_hint: Some(locator),
            raw_transaction_mutation_count: Some(declared_count),
            claims: claims(curve, mint, amount),
            raw_provider_role: Some(role),
            provenance: provenance(
                ObservationSourceFamilyV1::RawYellowstone,
                provider,
                hash_byte,
                10,
            ),
        }
    }

    fn nln(
        signature: Signature,
        locator_hint: Option<RawPumpMutationLocatorV1>,
        hash_byte: u8,
        curve: Pubkey,
        mint: Pubkey,
        amount: u64,
    ) -> ObservedPumpMutationV1 {
        ObservedPumpMutationV1 {
            mutation_family: PumpMutationFamilyV1::Trade,
            signature,
            locator_hint,
            canonical_order: None,
            raw_transaction_mutation_count: None,
            claims: claims(curve, mint, amount),
            raw_provider_role: None,
            provenance: provenance(ObservationSourceFamilyV1::ParsedNln, "nln", hash_byte, 20),
        }
    }

    #[test]
    fn raw_only_applies_immediately_and_tx_index_zero_survives() {
        let signature = Signature::new_unique();
        let locator = locator(signature, 0);
        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut ledger = PumpObservationLedgerV1::default();

        let result = ledger.observe(
            raw(
                locator,
                RawProviderRoleV1::PrimaryAuthority,
                "primary",
                1,
                curve,
                mint,
                10,
                1,
            ),
            1,
        );

        assert!(result.observation_decision.did_canonical_apply());
        assert_eq!(
            result
                .observation_decision
                .canonical_mutation
                .as_ref()
                .map(|mutation| mutation.order.tx_index),
            Some(0)
        );
        assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
    }

    #[test]
    fn exact_raw_nln_is_one_mutation_in_both_arrival_orders() {
        for nln_first in [false, true] {
            let signature = Signature::new_unique();
            let locator = locator(signature, 0);
            let curve = Pubkey::new_unique();
            let mint = Pubkey::new_unique();
            let raw = raw(
                locator.clone(),
                RawProviderRoleV1::PrimaryAuthority,
                "primary",
                1,
                curve,
                mint,
                10,
                1,
            );
            let nln = nln(signature, Some(locator), 2, curve, mint, 10);
            let mut ledger = PumpObservationLedgerV1::default();

            let (first, second) = if nln_first { (nln, raw) } else { (raw, nln) };
            let first_result = ledger.observe(first, 1);
            if nln_first {
                assert!(!first_result.observation_decision.did_canonical_apply());
            }
            let second_result = ledger.observe(second, 2);
            let all_decisions = std::iter::once(&second_result.observation_decision)
                .chain(second_result.derived_decisions.iter());
            assert!(all_decisions.clone().any(|decision| {
                decision.correlation
                    == Some(ParsedWitnessCorrelationOutcomeV1::ExactStructuralMatch)
                    && decision.provider_agreement
                        == PumpProviderAgreementV1::PrimarySecondaryAgreement
            }));
            assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
        }
    }

    #[test]
    fn locatorless_singleton_is_finalized_without_delaying_raw() {
        for nln_first in [false, true] {
            let signature = Signature::new_unique();
            let locator = locator(signature, 0);
            let curve = Pubkey::new_unique();
            let mint = Pubkey::new_unique();
            let raw = raw(
                locator,
                RawProviderRoleV1::PrimaryAuthority,
                "primary",
                1,
                curve,
                mint,
                10,
                1,
            );
            let nln = nln(signature, None, 2, curve, mint, 10);
            let mut ledger = PumpObservationLedgerV1::default();
            if nln_first {
                assert!(!ledger
                    .observe(nln, 1)
                    .observation_decision
                    .did_canonical_apply());
                assert!(ledger
                    .observe(raw, 2)
                    .observation_decision
                    .did_canonical_apply());
            } else {
                assert!(ledger
                    .observe(raw, 1)
                    .observation_decision
                    .did_canonical_apply());
                assert!(!ledger
                    .observe(nln, 2)
                    .observation_decision
                    .did_canonical_apply());
            }

            let decisions = ledger.finalize_expired(250_000_002);
            assert_eq!(decisions.len(), 1);
            assert_eq!(
                decisions[0].correlation,
                Some(ParsedWitnessCorrelationOutcomeV1::UniqueSignatureSingletonMatch)
            );
            assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
        }
    }

    #[test]
    fn multiple_mutations_same_signature_make_locatorless_witness_ambiguous() {
        let signature = Signature::new_unique();
        let first_locator = locator(signature, 0);
        let second_locator = RawPumpMutationLocatorV1 {
            semantic_event_ordinal: 1,
            ..first_locator.clone()
        };
        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut ledger = PumpObservationLedgerV1::default();
        assert!(ledger
            .observe(
                raw(
                    first_locator,
                    RawProviderRoleV1::PrimaryAuthority,
                    "primary",
                    1,
                    curve,
                    mint,
                    10,
                    2,
                ),
                1,
            )
            .observation_decision
            .did_canonical_apply());
        assert!(ledger
            .observe(
                raw(
                    second_locator,
                    RawProviderRoleV1::PrimaryAuthority,
                    "primary",
                    2,
                    curve,
                    mint,
                    20,
                    2,
                ),
                2,
            )
            .observation_decision
            .did_canonical_apply());
        ledger.observe(nln(signature, None, 3, curve, mint, 10), 3);

        let decisions = ledger.finalize_expired(250_000_003);
        assert_eq!(decisions.len(), 1);
        assert_eq!(
            decisions[0].correlation,
            Some(ParsedWitnessCorrelationOutcomeV1::Ambiguous)
        );
        assert_eq!(ledger.snapshot().canonical_mutation_count, 2);
    }

    #[test]
    fn unknown_claim_does_not_conflict_but_concrete_difference_does() {
        let signature = Signature::new_unique();
        let locator = locator(signature, 0);
        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut ledger = PumpObservationLedgerV1::default();
        ledger.observe(
            raw(
                locator.clone(),
                RawProviderRoleV1::PrimaryAuthority,
                "primary",
                1,
                curve,
                mint,
                10,
                1,
            ),
            1,
        );

        let mut unknown = nln(signature, Some(locator.clone()), 2, curve, mint, 10);
        unknown.claims.token_amount_units = None;
        let agreement = ledger.observe(unknown, 2).observation_decision;
        assert_eq!(
            agreement.provider_agreement,
            PumpProviderAgreementV1::PrimarySecondaryAgreement
        );

        let conflict = ledger
            .observe(nln(signature, Some(locator), 3, curve, mint, 11), 3)
            .observation_decision;
        assert_eq!(
            conflict.classification,
            PumpObservationClassificationV1::SourceReconciliationConflict
        );
        assert_eq!(
            conflict.conflict_fields,
            vec![PumpMutationConflictFieldV1::TokenAmountUnits]
        );
    }

    #[test]
    fn witness_saturation_never_vetoes_later_primary() {
        let config = PumpObservationLedgerConfigV1 {
            max_pending_witnesses: 1,
            max_primary_canonical_mutations: 2,
            max_correlated_witnesses_per_mutation: 1,
            max_retained_conflicts: 1,
            ..PumpObservationLedgerConfigV1::default()
        };
        let mut ledger = PumpObservationLedgerV1::try_new(config).expect("valid test config");
        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let first_signature = Signature::new_unique();
        let second_signature = Signature::new_unique();
        ledger.observe(nln(first_signature, None, 1, curve, mint, 1), 1);
        let overflow = ledger
            .observe(nln(second_signature, None, 2, curve, mint, 2), 2)
            .observation_decision;
        assert_eq!(
            overflow.classification,
            PumpObservationClassificationV1::EvidenceCapacityExceeded
        );

        let primary = ledger.observe(
            raw(
                locator(second_signature, 0),
                RawProviderRoleV1::PrimaryAuthority,
                "primary",
                3,
                curve,
                mint,
                2,
                1,
            ),
            3,
        );
        assert!(primary.observation_decision.did_canonical_apply());
        let snapshot = ledger.snapshot();
        assert_eq!(snapshot.canonical_mutation_count, 1);
        assert!(!snapshot.witness_evidence_complete);
        assert!(snapshot.first_evidence_overflow.is_some());
    }

    #[test]
    fn expired_secondary_reclaims_pending_capacity_without_singleton_or_canonical_authority() {
        let config = PumpObservationLedgerConfigV1 {
            correlation_window_ns: 10,
            max_pending_witnesses: 1,
            ..PumpObservationLedgerConfigV1::default()
        };
        let mut ledger = PumpObservationLedgerV1::try_new(config).expect("valid test config");
        let signature = Signature::new_unique();
        let locator = locator(signature, 0);
        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let secondary = raw(
            locator.clone(),
            RawProviderRoleV1::SecondaryWitness,
            "secondary",
            1,
            curve,
            mint,
            10,
            1,
        );

        let first = ledger.observe(secondary.clone(), 1).observation_decision;
        assert_eq!(
            first.classification,
            PumpObservationClassificationV1::SecondaryWitnessOnly
        );
        assert_eq!(ledger.snapshot().pending_witness_count, 1);

        let expired = ledger.finalize_expired(11);
        assert_eq!(expired.len(), 1);
        assert_eq!(
            expired[0].classification,
            PumpObservationClassificationV1::SecondaryWitnessExpired
        );
        assert_eq!(expired[0].correlation, None);
        assert!(expired[0].canonical_mutation.is_none());
        assert!(expired[0].candidate_integrity_signal.is_none());
        assert_eq!(ledger.snapshot().pending_witness_count, 0);
        assert_eq!(ledger.snapshot().canonical_mutation_count, 0);

        let replay = ledger.observe(secondary, 12).observation_decision;
        assert_eq!(
            replay.classification,
            PumpObservationClassificationV1::SecondaryWitnessOnly
        );
        let primary = ledger.observe(
            raw(
                locator,
                RawProviderRoleV1::PrimaryAuthority,
                "primary",
                2,
                curve,
                mint,
                10,
                1,
            ),
            13,
        );
        assert!(primary.observation_decision.did_canonical_apply());
        assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
    }

    #[test]
    fn secondary_same_or_different_hash_never_applies_canonical_twice() {
        let signature = Signature::new_unique();
        let locator = locator(signature, 0);
        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut ledger = PumpObservationLedgerV1::default();
        ledger.observe(
            raw(
                locator.clone(),
                RawProviderRoleV1::PrimaryAuthority,
                "primary",
                1,
                curve,
                mint,
                10,
                1,
            ),
            1,
        );
        let same = ledger
            .observe(
                raw(
                    locator.clone(),
                    RawProviderRoleV1::SecondaryWitness,
                    "secondary",
                    2,
                    curve,
                    mint,
                    10,
                    1,
                ),
                2,
            )
            .observation_decision;
        assert!(!same.did_canonical_apply());
        assert_eq!(
            same.provider_agreement,
            PumpProviderAgreementV1::PrimarySecondaryAgreement
        );
        let different = ledger
            .observe(
                raw(
                    locator,
                    RawProviderRoleV1::SecondaryWitness,
                    "secondary",
                    3,
                    curve,
                    mint,
                    11,
                    1,
                ),
                3,
            )
            .observation_decision;
        assert!(!different.did_canonical_apply());
        assert_eq!(
            different.provider_agreement,
            PumpProviderAgreementV1::PrimarySecondaryConflict
        );
        assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
    }
}
