//! Candidate-integrity lifecycle registry for PR1D.
//!
//! The registry is a technical fence around MFS/evaluation/submit.  It never
//! computes strategy features or changes a terminal Gatekeeper verdict.

use ghost_core::{
    CandidateIntegrityOutcomeV1, CandidateIntegritySignalV1, PumpCandidateIdentityV1,
    PumpMutationFamilyV1, RawPumpMutationLocatorV1, StructuralCanonicalPumpMutationV1,
};
use serde::{Deserialize, Serialize};
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CandidateIntegrityRegistryLimitsV1 {
    pub max_candidates: usize,
    pub max_audit_markers_per_candidate: usize,
}

impl Default for CandidateIntegrityRegistryLimitsV1 {
    fn default() -> Self {
        Self {
            max_candidates: 100_000,
            max_audit_markers_per_candidate: 32,
        }
    }
}

impl CandidateIntegrityRegistryLimitsV1 {
    fn normalized(self) -> Self {
        Self {
            max_candidates: self.max_candidates.max(1),
            max_audit_markers_per_candidate: self.max_audit_markers_per_candidate.max(1),
        }
    }
}

/// Real lifecycle phase used by the technical integrity conflict matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateLifecyclePhaseV1 {
    PreMfs,
    MfsMaterialized,
    EvaluationRunning,
    TerminalReject,
    TerminalTimeout,
    TerminalBuyNotSubmitted,
    SubmitStarted,
    ConfirmedOpenPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateTerminalTransitionV1 {
    Reject,
    Timeout,
    BuyNotSubmitted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateIntegrityConflictActionV1 {
    ReadyRegistered,
    DuplicateReady,
    ExistingFailurePreserved,
    BlockBeforeMfs,
    InterruptEvaluation,
    TerminalVerdictImmutableAudit,
    CancelExecutionBeforeSubmit,
    ReconciliationRequired,
    ConfirmedPositionQuarantined,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateIntegrityAuditMarkerV1 {
    pub generation: u64,
    pub outcome: CandidateIntegrityOutcomeV1,
    pub phase_at_observation: CandidateLifecyclePhaseV1,
    pub evidence_hash_blake3: [u8; 32],
    pub action: CandidateIntegrityConflictActionV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateIntegrityRecordV1 {
    pub candidate: PumpCandidateIdentityV1,
    pub outcome: CandidateIntegrityOutcomeV1,
    pub generation: u64,
    pub lifecycle_phase: CandidateLifecyclePhaseV1,
    pub reconciliation_required: bool,
    pub witness_quarantined: bool,
    pub audit_evidence_complete: bool,
    #[serde(default)]
    pub audit_evidence_overflow_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_rejected_audit_marker: Option<CandidateIntegrityAuditMarkerV1>,
    pub audit_markers: Vec<CandidateIntegrityAuditMarkerV1>,
}

#[derive(Debug, Default)]
struct CandidateIntegrityRegistryStateV1 {
    records: HashMap<PumpCandidateIdentityV1, CandidateIntegrityRecordV1>,
    by_pool: HashMap<solana_sdk::pubkey::Pubkey, PumpCandidateIdentityV1>,
    by_mint: HashMap<solana_sdk::pubkey::Pubkey, PumpCandidateIdentityV1>,
    canonical_apply_fence: CanonicalApplyFenceV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RuntimeMutationApplyKeyV1 {
    mutation_family: PumpMutationFamilyV1,
    signature: Signature,
    candidate: PumpCandidateIdentityV1,
    semantic_event_ordinal: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalMutationApplyReceiptV1 {
    runtime_key: RuntimeMutationApplyKeyV1,
    pub(crate) signature: Signature,
    pub(crate) locator: RawPumpMutationLocatorV1,
    pub(crate) candidate: PumpCandidateIdentityV1,
    evidence_hash_blake3: [u8; 32],
}

#[derive(Clone, Debug)]
struct CanonicalApplyReceiptStateV1 {
    receipt: CanonicalMutationApplyReceiptV1,
    applied: bool,
    failed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CandidateIntegrityCasTokenV1 {
    Absent,
    Generation(u64),
}

#[derive(Clone, Debug)]
struct CandidateApplyProofV1 {
    integrity_cas: CandidateIntegrityCasTokenV1,
    expected_locators: HashSet<RawPumpMutationLocatorV1>,
    ready_signal: CandidateIntegritySignalV1,
    invalidated: bool,
    ready_published: bool,
}

#[derive(Debug, Default)]
struct CanonicalApplyFenceV1 {
    receipts_by_runtime_key: HashMap<RuntimeMutationApplyKeyV1, CanonicalApplyReceiptStateV1>,
    proofs_by_signature_candidate:
        HashMap<(Signature, PumpCandidateIdentityV1), CandidateApplyProofV1>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CandidateIntegrityReadyReleaseV1 {
    pub(crate) candidate: PumpCandidateIdentityV1,
    pub(crate) signature: Signature,
    pub(crate) locator: RawPumpMutationLocatorV1,
    pub(crate) evidence_hash_blake3: [u8; 32],
}

#[derive(Debug)]
pub struct CandidateIntegrityRegistry {
    limits: CandidateIntegrityRegistryLimitsV1,
    state: Mutex<CandidateIntegrityRegistryStateV1>,
    available: AtomicBool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateIntegritySignalResultV1 {
    pub action: CandidateIntegrityConflictActionV1,
    pub snapshot: CandidateIntegrityRecordV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum CandidateIntegrityErrorV1 {
    #[error("candidate integrity registry mutex is poisoned")]
    RegistryUnavailable,
    #[error("candidate integrity registry capacity exceeded")]
    RegistryCapacityExceeded,
    #[error("candidate integrity record is missing")]
    CandidateMissing,
    #[error("candidate identity aliases disagree")]
    CandidateAliasConflict,
    #[error("candidate integrity is not Ready: {0:?}")]
    NotReady(CandidateIntegrityOutcomeV1),
    #[error("candidate integrity generation changed: expected={expected} actual={actual}")]
    GenerationChanged { expected: u64, actual: u64 },
    #[error("candidate lifecycle phase mismatch: expected={expected:?} actual={actual:?}")]
    PhaseMismatch {
        expected: CandidateLifecyclePhaseV1,
        actual: CandidateLifecyclePhaseV1,
    },
}

impl CandidateIntegrityRegistry {
    fn failure_record_capacity(&self) -> usize {
        self.limits.max_candidates.saturating_mul(2)
    }

    fn coverage_incomplete_signal(
        receipt: &CanonicalMutationApplyReceiptV1,
    ) -> CandidateIntegritySignalV1 {
        CandidateIntegritySignalV1 {
            candidate: receipt.candidate,
            outcome: CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
            signature: Some(receipt.signature),
            locator: Some(receipt.locator.clone()),
            conflict_fields: Vec::new(),
            evidence_hash_blake3: receipt.evidence_hash_blake3,
        }
    }

    fn cleanup_canonical_apply_fence_for_candidate(
        state: &mut CandidateIntegrityRegistryStateV1,
        candidate: PumpCandidateIdentityV1,
    ) {
        for ((_, proof_candidate), proof) in state
            .canonical_apply_fence
            .proofs_by_signature_candidate
            .iter_mut()
        {
            if *proof_candidate == candidate {
                proof.invalidated = true;
            }
        }
        if state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .values()
            .any(|entry| entry.receipt.candidate == candidate && !entry.applied && !entry.failed)
        {
            return;
        }
        state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .retain(|_, entry| entry.receipt.candidate != candidate);
        state
            .canonical_apply_fence
            .proofs_by_signature_candidate
            .retain(|(_, proof_candidate), _| *proof_candidate != candidate);
    }

    #[must_use]
    pub fn new(limits: CandidateIntegrityRegistryLimitsV1) -> Self {
        Self {
            limits: limits.normalized(),
            state: Mutex::new(CandidateIntegrityRegistryStateV1::default()),
            available: AtomicBool::new(true),
        }
    }

    pub(crate) fn stage_canonical_mutation(
        &self,
        canonical: &StructuralCanonicalPumpMutationV1,
    ) -> Result<CanonicalMutationApplyReceiptV1, CandidateIntegrityErrorV1> {
        self.require_available()?;
        let candidate = PumpCandidateIdentityV1 {
            pool_amm_id: canonical
                .claims
                .curve
                .ok_or(CandidateIntegrityErrorV1::CandidateMissing)?,
            mint: canonical
                .claims
                .mint
                .ok_or(CandidateIntegrityErrorV1::CandidateMissing)?,
        };
        let runtime_key = RuntimeMutationApplyKeyV1 {
            mutation_family: canonical.mutation_family,
            signature: canonical.locator.signature,
            candidate,
            semantic_event_ordinal: canonical.locator.semantic_event_ordinal,
        };
        let receipt = CanonicalMutationApplyReceiptV1 {
            runtime_key: runtime_key.clone(),
            signature: canonical.locator.signature,
            locator: canonical.locator.clone(),
            candidate,
            evidence_hash_blake3: canonical.primary_raw_provenance.payload_hash_blake3,
        };
        let mut state = self.lock_state()?;
        if let Some(existing) = state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .get(&runtime_key)
        {
            if existing.receipt == receipt {
                return Ok(existing.receipt.clone());
            }
            self.available.store(false, Ordering::Release);
            return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
        }
        if state.canonical_apply_fence.receipts_by_runtime_key.len() >= self.limits.max_candidates {
            drop(state);
            let _ = self.record_signal(Self::coverage_incomplete_signal(&receipt))?;
            return Err(CandidateIntegrityErrorV1::RegistryCapacityExceeded);
        }
        state.canonical_apply_fence.receipts_by_runtime_key.insert(
            runtime_key,
            CanonicalApplyReceiptStateV1 {
                receipt: receipt.clone(),
                applied: false,
                failed: false,
            },
        );
        Ok(receipt)
    }

    pub(crate) fn seal_complete_transaction_inventory(
        &self,
        signature: Signature,
        ready_signals: &[CandidateIntegritySignalV1],
    ) -> Result<(), CandidateIntegrityErrorV1> {
        self.require_available()?;
        let mut state = self.lock_state()?;
        let mut pending_proofs = Vec::new();
        for signal in ready_signals {
            if signal.outcome != CandidateIntegrityOutcomeV1::Ready
                || signal.signature != Some(signature)
            {
                continue;
            }
            validate_aliases(&state, signal.candidate)?;
            let expected_locators = state
                .canonical_apply_fence
                .receipts_by_runtime_key
                .values()
                .filter(|entry| {
                    entry.receipt.signature == signature
                        && entry.receipt.candidate == signal.candidate
                })
                .map(|entry| entry.receipt.locator.clone())
                .collect::<HashSet<_>>();
            if expected_locators.is_empty() {
                self.available.store(false, Ordering::Release);
                return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
            }
            let invalidated = state
                .canonical_apply_fence
                .receipts_by_runtime_key
                .values()
                .filter(|entry| {
                    entry.receipt.signature == signature
                        && entry.receipt.candidate == signal.candidate
                })
                .any(|entry| entry.failed)
                || state
                    .records
                    .get(&signal.candidate)
                    .is_some_and(|record| record.outcome != CandidateIntegrityOutcomeV1::Ready);
            let integrity_cas = state
                .records
                .get(&signal.candidate)
                .map_or(CandidateIntegrityCasTokenV1::Absent, |record| {
                    CandidateIntegrityCasTokenV1::Generation(record.generation)
                });
            let key = (signature, signal.candidate);
            if let Some(existing) = state
                .canonical_apply_fence
                .proofs_by_signature_candidate
                .get(&key)
            {
                if existing.expected_locators != expected_locators
                    || existing.ready_signal != *signal
                {
                    self.available.store(false, Ordering::Release);
                    return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
                }
                continue;
            }
            if state
                .canonical_apply_fence
                .proofs_by_signature_candidate
                .len()
                + pending_proofs.len()
                >= self.limits.max_candidates
            {
                let failure_signals = ready_signals
                    .iter()
                    .filter(|candidate_signal| {
                        candidate_signal.outcome == CandidateIntegrityOutcomeV1::Ready
                            && candidate_signal.signature == Some(signature)
                    })
                    .map(|candidate_signal| CandidateIntegritySignalV1 {
                        candidate: candidate_signal.candidate,
                        outcome: CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                        signature: Some(signature),
                        locator: candidate_signal.locator.clone().or_else(|| {
                            state
                                .canonical_apply_fence
                                .receipts_by_runtime_key
                                .values()
                                .find(|entry| {
                                    entry.receipt.signature == signature
                                        && entry.receipt.candidate == candidate_signal.candidate
                                })
                                .map(|entry| entry.receipt.locator.clone())
                        }),
                        conflict_fields: Vec::new(),
                        evidence_hash_blake3: candidate_signal.evidence_hash_blake3,
                    })
                    .collect::<Vec<_>>();
                for entry in state
                    .canonical_apply_fence
                    .receipts_by_runtime_key
                    .values_mut()
                {
                    if failure_signals.iter().any(|failure| {
                        entry.receipt.signature == signature
                            && entry.receipt.candidate == failure.candidate
                    }) {
                        entry.failed = true;
                    }
                }
                drop(state);
                for failure in failure_signals {
                    let _ = self.record_signal(failure)?;
                }
                return Err(CandidateIntegrityErrorV1::RegistryCapacityExceeded);
            }
            pending_proofs.push((
                key,
                CandidateApplyProofV1 {
                    integrity_cas,
                    expected_locators,
                    ready_signal: signal.clone(),
                    invalidated,
                    ready_published: false,
                },
            ));
        }
        for (key, proof) in pending_proofs {
            state
                .canonical_apply_fence
                .proofs_by_signature_candidate
                .insert(key, proof);
        }
        Ok(())
    }

    pub(crate) fn trade_apply_receipt(
        &self,
        signature: Signature,
        candidate: PumpCandidateIdentityV1,
        semantic_event_ordinal: u32,
    ) -> Result<Option<CanonicalMutationApplyReceiptV1>, CandidateIntegrityErrorV1> {
        self.apply_receipt_for_key(
            PumpMutationFamilyV1::Trade,
            signature,
            candidate,
            Some(semantic_event_ordinal),
        )
    }

    pub(crate) fn initialize_pool_apply_receipt(
        &self,
        signature: Signature,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<Option<CanonicalMutationApplyReceiptV1>, CandidateIntegrityErrorV1> {
        self.apply_receipt_for_key(
            PumpMutationFamilyV1::InitializePool,
            signature,
            candidate,
            None,
        )
    }

    fn apply_receipt_for_key(
        &self,
        mutation_family: PumpMutationFamilyV1,
        signature: Signature,
        candidate: PumpCandidateIdentityV1,
        semantic_event_ordinal: Option<u32>,
    ) -> Result<Option<CanonicalMutationApplyReceiptV1>, CandidateIntegrityErrorV1> {
        self.require_available()?;
        let state = self.lock_state()?;
        let mut matches = state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .values()
            .filter(|entry| {
                entry.receipt.runtime_key.mutation_family == mutation_family
                    && entry.receipt.signature == signature
                    && entry.receipt.candidate == candidate
                    && semantic_event_ordinal.is_none_or(|ordinal| {
                        entry.receipt.runtime_key.semantic_event_ordinal == ordinal
                    })
            });
        let first = matches.next().map(|entry| entry.receipt.clone());
        if matches.next().is_some() {
            return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
        }
        Ok(first)
    }

    pub(crate) fn fail_canonical_apply(
        &self,
        receipt: &CanonicalMutationApplyReceiptV1,
    ) -> Result<(), CandidateIntegrityErrorV1> {
        self.require_available()?;
        {
            let mut state = self.lock_state()?;
            let entry = state
                .canonical_apply_fence
                .receipts_by_runtime_key
                .get_mut(&receipt.runtime_key)
                .ok_or(CandidateIntegrityErrorV1::CandidateMissing)?;
            if entry.receipt != *receipt {
                self.available.store(false, Ordering::Release);
                return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
            }
            if entry.applied || entry.failed {
                return Ok(());
            }
            entry.failed = true;
            if let Some(proof) = state
                .canonical_apply_fence
                .proofs_by_signature_candidate
                .get_mut(&(receipt.signature, receipt.candidate))
            {
                proof.invalidated = true;
            }
        }
        let _ = self.record_signal(Self::coverage_incomplete_signal(receipt))?;
        Ok(())
    }

    pub(crate) fn fail_ready_release(
        &self,
        release: &CandidateIntegrityReadyReleaseV1,
    ) -> Result<(), CandidateIntegrityErrorV1> {
        let _ = self.record_signal(CandidateIntegritySignalV1 {
            candidate: release.candidate,
            outcome: CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
            signature: Some(release.signature),
            locator: Some(release.locator.clone()),
            conflict_fields: Vec::new(),
            evidence_hash_blake3: release.evidence_hash_blake3,
        })?;
        Ok(())
    }

    pub(crate) fn invalidate_pending_canonical_applies(
        &self,
    ) -> Result<(), CandidateIntegrityErrorV1> {
        self.require_available()?;
        let signals = {
            let mut state = self.lock_state()?;
            let pending = state
                .canonical_apply_fence
                .receipts_by_runtime_key
                .values_mut()
                .filter(|entry| !entry.applied && !entry.failed)
                .map(|entry| {
                    entry.failed = true;
                    CandidateIntegritySignalV1 {
                        candidate: entry.receipt.candidate,
                        outcome: CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                        signature: Some(entry.receipt.signature),
                        locator: Some(entry.receipt.locator.clone()),
                        conflict_fields: Vec::new(),
                        evidence_hash_blake3: entry.receipt.evidence_hash_blake3,
                    }
                })
                .collect::<Vec<_>>();
            for proof in state
                .canonical_apply_fence
                .proofs_by_signature_candidate
                .values_mut()
            {
                proof.invalidated = true;
            }
            pending
        };
        let mut seen = HashSet::new();
        for signal in signals {
            if seen.insert((signal.candidate, signal.signature)) {
                let _ = self.record_signal(signal)?;
            }
        }
        Ok(())
    }

    pub(crate) fn mark_canonical_apply_succeeded(
        &self,
        receipt: &CanonicalMutationApplyReceiptV1,
    ) -> Result<Vec<CandidateIntegrityReadyReleaseV1>, CandidateIntegrityErrorV1> {
        self.require_available()?;
        let mut state = self.lock_state()?;
        let entry = state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .get_mut(&receipt.runtime_key)
            .ok_or(CandidateIntegrityErrorV1::CandidateMissing)?;
        if entry.receipt != *receipt {
            self.available.store(false, Ordering::Release);
            return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
        }
        if entry.failed || entry.applied {
            return Ok(Vec::new());
        }

        let proof_keys = state
            .canonical_apply_fence
            .proofs_by_signature_candidate
            .keys()
            .filter(|(signature, _)| *signature == receipt.signature)
            .copied()
            .collect::<Vec<_>>();
        let mut eligible = Vec::new();
        for key in proof_keys {
            let Some(proof) = state
                .canonical_apply_fence
                .proofs_by_signature_candidate
                .get(&key)
            else {
                continue;
            };
            if proof.invalidated || proof.ready_published {
                continue;
            }
            let all_applied = proof.expected_locators.iter().all(|locator| {
                state
                    .canonical_apply_fence
                    .receipts_by_runtime_key
                    .values()
                    .any(|entry| {
                        entry.receipt.locator == *locator
                            && !entry.failed
                            && (entry.applied || entry.receipt.runtime_key == receipt.runtime_key)
                    })
            });
            if all_applied {
                eligible.push((key, proof.integrity_cas, proof.ready_signal.clone()));
            }
        }

        let new_ready_records = eligible
            .iter()
            .filter(|(_, cas, _)| *cas == CandidateIntegrityCasTokenV1::Absent)
            .filter(|(_, _, signal)| !state.records.contains_key(&signal.candidate))
            .count();
        if let Some(error) = eligible
            .iter()
            .find_map(|(_, _, signal)| validate_aliases(&state, signal.candidate).err())
        {
            let failure_signals = eligible
                .iter()
                .map(|(_, _, signal)| CandidateIntegritySignalV1 {
                    candidate: signal.candidate,
                    outcome: CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                    signature: signal.signature,
                    locator: signal.locator.clone(),
                    conflict_fields: Vec::new(),
                    evidence_hash_blake3: signal.evidence_hash_blake3,
                })
                .collect::<Vec<_>>();
            if let Some(entry) = state
                .canonical_apply_fence
                .receipts_by_runtime_key
                .get_mut(&receipt.runtime_key)
            {
                entry.failed = true;
            }
            for signal in &failure_signals {
                Self::cleanup_canonical_apply_fence_for_candidate(&mut state, signal.candidate);
            }
            drop(state);
            for signal in failure_signals {
                let _ = self.record_signal(signal);
            }
            return Err(error);
        }
        if new_ready_records
            > self
                .limits
                .max_candidates
                .saturating_sub(state.records.len())
        {
            let failure_signals = eligible
                .iter()
                .map(|(_, _, signal)| CandidateIntegritySignalV1 {
                    candidate: signal.candidate,
                    outcome: CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                    signature: signal.signature,
                    locator: signal.locator.clone(),
                    conflict_fields: Vec::new(),
                    evidence_hash_blake3: signal.evidence_hash_blake3,
                })
                .collect::<Vec<_>>();
            if let Some(entry) = state
                .canonical_apply_fence
                .receipts_by_runtime_key
                .get_mut(&receipt.runtime_key)
            {
                entry.failed = true;
            }
            for (key, _, _) in &eligible {
                if let Some(proof) = state
                    .canonical_apply_fence
                    .proofs_by_signature_candidate
                    .get_mut(key)
                {
                    proof.invalidated = true;
                }
            }
            drop(state);
            for signal in failure_signals {
                let _ = self.record_signal(signal)?;
            }
            return Err(CandidateIntegrityErrorV1::RegistryCapacityExceeded);
        }
        let Some(entry) = state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .get_mut(&receipt.runtime_key)
        else {
            self.available.store(false, Ordering::Release);
            return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
        };
        entry.applied = true;

        let mut released = Vec::new();
        for (key, cas, signal) in eligible {
            let published_now = publish_ready_with_cas(&mut state, self.limits, cas, &signal)?;
            let already_ready = state
                .records
                .get(&signal.candidate)
                .is_some_and(|record| record.outcome == CandidateIntegrityOutcomeV1::Ready);
            if let Some(proof) = state
                .canonical_apply_fence
                .proofs_by_signature_candidate
                .get_mut(&key)
            {
                proof.ready_published = published_now || already_ready;
            }
            if published_now {
                released.push(CandidateIntegrityReadyReleaseV1 {
                    candidate: signal.candidate,
                    signature: signal.signature.unwrap_or(receipt.signature),
                    locator: signal
                        .locator
                        .clone()
                        .unwrap_or_else(|| receipt.locator.clone()),
                    evidence_hash_blake3: signal.evidence_hash_blake3,
                });
            }
        }
        if state.records.get(&receipt.candidate).is_some_and(|record| {
            record.outcome != CandidateIntegrityOutcomeV1::Ready
                || matches!(
                    record.lifecycle_phase,
                    CandidateLifecyclePhaseV1::TerminalReject
                        | CandidateLifecyclePhaseV1::TerminalTimeout
                        | CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted
                        | CandidateLifecyclePhaseV1::ConfirmedOpenPosition
                )
        }) {
            Self::cleanup_canonical_apply_fence_for_candidate(&mut state, receipt.candidate);
        }
        Ok(released)
    }

    pub fn record_signal(
        &self,
        signal: CandidateIntegritySignalV1,
    ) -> Result<CandidateIntegritySignalResultV1, CandidateIntegrityErrorV1> {
        self.require_available()?;
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.available.store(false, Ordering::Release);
                return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
            }
        };
        let alias_conflicts = conflicting_alias_candidates(&state, signal.candidate);
        if !alias_conflicts.is_empty() {
            for existing_candidate in alias_conflicts.iter().copied() {
                if let Some(record) = state.records.get_mut(&existing_candidate) {
                    record.generation = record.generation.saturating_add(1);
                    record.outcome = CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete;
                    let action = conflict_action_for_phase(record);
                    append_audit_marker(
                        record,
                        self.limits.max_audit_markers_per_candidate,
                        CandidateIntegrityAuditMarkerV1 {
                            generation: record.generation,
                            outcome: CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                            phase_at_observation: record.lifecycle_phase,
                            evidence_hash_blake3: signal.evidence_hash_blake3,
                            action,
                        },
                    );
                }
            }
            for existing_candidate in alias_conflicts {
                Self::cleanup_canonical_apply_fence_for_candidate(&mut state, existing_candidate);
            }
            return Err(CandidateIntegrityErrorV1::CandidateAliasConflict);
        }

        let inserted = !state.records.contains_key(&signal.candidate);
        if inserted {
            let capacity =
                if signal.outcome == CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete {
                    self.failure_record_capacity()
                } else {
                    self.limits.max_candidates
                };
            if state.records.len() >= capacity {
                self.available.store(false, Ordering::Release);
                return Err(CandidateIntegrityErrorV1::RegistryCapacityExceeded);
            }
            state
                .by_pool
                .insert(signal.candidate.pool_amm_id, signal.candidate);
            state
                .by_mint
                .insert(signal.candidate.mint, signal.candidate);
            state.records.insert(
                signal.candidate,
                CandidateIntegrityRecordV1 {
                    candidate: signal.candidate,
                    outcome: signal.outcome,
                    generation: 1,
                    lifecycle_phase: CandidateLifecyclePhaseV1::PreMfs,
                    reconciliation_required: false,
                    witness_quarantined: false,
                    audit_evidence_complete: true,
                    audit_evidence_overflow_count: 0,
                    first_rejected_audit_marker: None,
                    audit_markers: Vec::new(),
                },
            );
        }

        let record = state
            .records
            .get_mut(&signal.candidate)
            .ok_or(CandidateIntegrityErrorV1::CandidateMissing)?;
        let action = if signal.outcome == CandidateIntegrityOutcomeV1::Ready {
            if record.outcome == CandidateIntegrityOutcomeV1::Ready {
                CandidateIntegrityConflictActionV1::DuplicateReady
            } else {
                CandidateIntegrityConflictActionV1::ExistingFailurePreserved
            }
        } else {
            if !inserted {
                record.generation = record.generation.saturating_add(1);
            }
            record.outcome = signal.outcome;
            conflict_action_for_phase(record)
        };
        let action = if record.audit_markers.is_empty()
            && signal.outcome == CandidateIntegrityOutcomeV1::Ready
        {
            CandidateIntegrityConflictActionV1::ReadyRegistered
        } else {
            action
        };
        if action != CandidateIntegrityConflictActionV1::DuplicateReady {
            append_audit_marker(
                record,
                self.limits.max_audit_markers_per_candidate,
                CandidateIntegrityAuditMarkerV1 {
                    generation: record.generation,
                    outcome: signal.outcome,
                    phase_at_observation: record.lifecycle_phase,
                    evidence_hash_blake3: signal.evidence_hash_blake3,
                    action,
                },
            );
        }

        let result = CandidateIntegritySignalResultV1 {
            action,
            snapshot: record.clone(),
        };
        if signal.outcome != CandidateIntegrityOutcomeV1::Ready {
            Self::cleanup_canonical_apply_fence_for_candidate(&mut state, signal.candidate);
        }
        Ok(result)
    }

    pub fn evaluation_guard(
        self: &Arc<Self>,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<CandidateIntegrityEvaluationGuardV1, CandidateIntegrityErrorV1> {
        self.require_available()?;
        let state = self.lock_state()?;
        let record = lookup_record(&state, candidate)?;
        require_ready(record)?;
        if record.lifecycle_phase != CandidateLifecyclePhaseV1::PreMfs {
            return Err(CandidateIntegrityErrorV1::PhaseMismatch {
                expected: CandidateLifecyclePhaseV1::PreMfs,
                actual: record.lifecycle_phase,
            });
        }
        Ok(CandidateIntegrityEvaluationGuardV1 {
            registry: Arc::clone(self),
            candidate: record.candidate,
            generation: record.generation,
        })
    }

    pub fn submit_guard(
        self: &Arc<Self>,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<CandidateIntegritySubmitGuardV1, CandidateIntegrityErrorV1> {
        self.require_available()?;
        let state = self.lock_state()?;
        let record = lookup_record(&state, candidate)?;
        require_ready(record)?;
        if record.lifecycle_phase != CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted {
            return Err(CandidateIntegrityErrorV1::PhaseMismatch {
                expected: CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted,
                actual: record.lifecycle_phase,
            });
        }
        Ok(CandidateIntegritySubmitGuardV1 {
            registry: Arc::clone(self),
            candidate: record.candidate,
            generation: record.generation,
            submit_started: Arc::new(AtomicBool::new(false)),
        })
    }

    #[must_use]
    pub fn snapshot(
        &self,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<CandidateIntegrityRecordV1, CandidateIntegrityErrorV1> {
        let state = self.lock_state()?;
        Ok(lookup_record(&state, candidate)?.clone())
    }

    /// Return the current CandidateIntegrity authority status for canonical
    /// AccountStateCore mutation.
    ///
    /// `None` means that no PR1D record exists for this legacy candidate and
    /// preserves pre-PR1D compatibility. `Some(false)` is authoritative: a
    /// technical integrity failure or terminal reject/timeout has already
    /// closed canonical account mutation, so later observations are
    /// evidence-only. Registry unavailability is returned as an error and
    /// callers must fail closed.
    pub fn account_state_apply_allowed(
        &self,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<Option<bool>, CandidateIntegrityErrorV1> {
        self.require_available()?;
        let state = self.lock_state()?;
        validate_aliases(&state, candidate)?;
        let Some(record) = state.records.get(&candidate) else {
            return Ok(None);
        };
        Ok(Some(
            record.outcome == CandidateIntegrityOutcomeV1::Ready
                && !matches!(
                    record.lifecycle_phase,
                    CandidateLifecyclePhaseV1::TerminalReject
                        | CandidateLifecyclePhaseV1::TerminalTimeout
                ),
        ))
    }

    #[must_use]
    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::Acquire)
    }

    fn require_available(&self) -> Result<(), CandidateIntegrityErrorV1> {
        if self.is_available() {
            Ok(())
        } else {
            Err(CandidateIntegrityErrorV1::RegistryUnavailable)
        }
    }

    fn lock_state(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, CandidateIntegrityRegistryStateV1>,
        CandidateIntegrityErrorV1,
    > {
        match self.state.lock() {
            Ok(state) => Ok(state),
            Err(_) => {
                self.available.store(false, Ordering::Release);
                Err(CandidateIntegrityErrorV1::RegistryUnavailable)
            }
        }
    }

    fn transition_guard_phase(
        &self,
        candidate: PumpCandidateIdentityV1,
        generation: u64,
        expected: CandidateLifecyclePhaseV1,
        next: CandidateLifecyclePhaseV1,
    ) -> Result<(), CandidateIntegrityErrorV1> {
        self.require_available()?;
        let mut state = self.lock_state()?;
        let record = lookup_record_mut(&mut state, candidate)?;
        require_generation(record, generation)?;
        require_ready(record)?;
        if record.lifecycle_phase != expected {
            return Err(CandidateIntegrityErrorV1::PhaseMismatch {
                expected,
                actual: record.lifecycle_phase,
            });
        }
        record.lifecycle_phase = next;
        if matches!(
            next,
            CandidateLifecyclePhaseV1::TerminalReject
                | CandidateLifecyclePhaseV1::TerminalTimeout
                | CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted
                | CandidateLifecyclePhaseV1::ConfirmedOpenPosition
        ) {
            Self::cleanup_canonical_apply_fence_for_candidate(&mut state, candidate);
        }
        Ok(())
    }

    fn check_guard(
        &self,
        candidate: PumpCandidateIdentityV1,
        generation: u64,
    ) -> Result<(), CandidateIntegrityErrorV1> {
        self.require_available()?;
        let state = self.lock_state()?;
        let record = lookup_record(&state, candidate)?;
        require_generation(record, generation)?;
        require_ready(record)
    }
}

impl Default for CandidateIntegrityRegistry {
    fn default() -> Self {
        Self::new(CandidateIntegrityRegistryLimitsV1::default())
    }
}

#[derive(Clone, Debug)]
pub struct CandidateIntegrityEvaluationGuardV1 {
    registry: Arc<CandidateIntegrityRegistry>,
    candidate: PumpCandidateIdentityV1,
    generation: u64,
}

impl CandidateIntegrityEvaluationGuardV1 {
    #[must_use]
    pub const fn candidate(&self) -> PumpCandidateIdentityV1 {
        self.candidate
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn check_ready(&self) -> Result<(), CandidateIntegrityErrorV1> {
        self.registry.check_guard(self.candidate, self.generation)
    }

    pub fn mark_mfs_materialized(&self) -> Result<(), CandidateIntegrityErrorV1> {
        self.registry.transition_guard_phase(
            self.candidate,
            self.generation,
            CandidateLifecyclePhaseV1::PreMfs,
            CandidateLifecyclePhaseV1::MfsMaterialized,
        )
    }

    pub fn mark_evaluation_running(&self) -> Result<(), CandidateIntegrityErrorV1> {
        self.registry.transition_guard_phase(
            self.candidate,
            self.generation,
            CandidateLifecyclePhaseV1::MfsMaterialized,
            CandidateLifecyclePhaseV1::EvaluationRunning,
        )
    }

    pub fn reset_pre_mfs(&self) -> Result<(), CandidateIntegrityErrorV1> {
        self.registry.require_available()?;
        let mut state = self.registry.lock_state()?;
        let record = lookup_record_mut(&mut state, self.candidate)?;
        require_generation(record, self.generation)?;
        require_ready(record)?;
        if !matches!(
            record.lifecycle_phase,
            CandidateLifecyclePhaseV1::MfsMaterialized
                | CandidateLifecyclePhaseV1::EvaluationRunning
        ) {
            return Err(CandidateIntegrityErrorV1::PhaseMismatch {
                expected: CandidateLifecyclePhaseV1::EvaluationRunning,
                actual: record.lifecycle_phase,
            });
        }
        record.lifecycle_phase = CandidateLifecyclePhaseV1::PreMfs;
        Ok(())
    }

    pub fn publish_terminal(
        &self,
        terminal: CandidateTerminalTransitionV1,
    ) -> Result<Option<CandidateIntegritySubmitGuardV1>, CandidateIntegrityErrorV1> {
        let next = match terminal {
            CandidateTerminalTransitionV1::Reject => CandidateLifecyclePhaseV1::TerminalReject,
            CandidateTerminalTransitionV1::Timeout => CandidateLifecyclePhaseV1::TerminalTimeout,
            CandidateTerminalTransitionV1::BuyNotSubmitted => {
                CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted
            }
        };
        self.registry.transition_guard_phase(
            self.candidate,
            self.generation,
            CandidateLifecyclePhaseV1::EvaluationRunning,
            next,
        )?;
        Ok(
            (terminal == CandidateTerminalTransitionV1::BuyNotSubmitted).then(|| {
                CandidateIntegritySubmitGuardV1 {
                    registry: Arc::clone(&self.registry),
                    candidate: self.candidate,
                    generation: self.generation,
                    submit_started: Arc::new(AtomicBool::new(false)),
                }
            }),
        )
    }
}

#[derive(Clone, Debug)]
pub struct CandidateIntegritySubmitGuardV1 {
    registry: Arc<CandidateIntegrityRegistry>,
    candidate: PumpCandidateIdentityV1,
    generation: u64,
    submit_started: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateSubmitTransitionV1 {
    StartedNow,
    AlreadyStarted,
}

impl CandidateIntegritySubmitGuardV1 {
    pub fn try_begin_submit(
        &self,
    ) -> Result<CandidateSubmitTransitionV1, CandidateIntegrityErrorV1> {
        if self.submit_started.load(Ordering::Acquire) {
            return Ok(CandidateSubmitTransitionV1::AlreadyStarted);
        }
        self.registry.transition_guard_phase(
            self.candidate,
            self.generation,
            CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted,
            CandidateLifecyclePhaseV1::SubmitStarted,
        )?;
        self.submit_started.store(true, Ordering::Release);
        Ok(CandidateSubmitTransitionV1::StartedNow)
    }

    pub fn mark_confirmed(&self) -> Result<(), CandidateIntegrityErrorV1> {
        if !self.submit_started.load(Ordering::Acquire) {
            return Err(CandidateIntegrityErrorV1::PhaseMismatch {
                expected: CandidateLifecyclePhaseV1::SubmitStarted,
                actual: CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted,
            });
        }
        let mut state = self
            .registry
            .state
            .lock()
            .map_err(|_| CandidateIntegrityErrorV1::RegistryUnavailable)?;
        let record = lookup_record_mut(&mut state, self.candidate)?;
        if record.lifecycle_phase != CandidateLifecyclePhaseV1::SubmitStarted {
            return Err(CandidateIntegrityErrorV1::PhaseMismatch {
                expected: CandidateLifecyclePhaseV1::SubmitStarted,
                actual: record.lifecycle_phase,
            });
        }
        // A conflict after submit deliberately changes generation/outcome but
        // must not prevent confirmation from establishing the real position.
        record.lifecycle_phase = CandidateLifecyclePhaseV1::ConfirmedOpenPosition;
        CandidateIntegrityRegistry::cleanup_canonical_apply_fence_for_candidate(
            &mut state,
            self.candidate,
        );
        Ok(())
    }

    /// Whether a sender-started attempt must retain capacity and enter
    /// reconciliation. Registry unavailability is conservatively treated as
    /// requiring reconciliation.
    #[must_use]
    pub fn requires_reconciliation(&self) -> bool {
        if !self.submit_started.load(Ordering::Acquire) {
            return false;
        }
        self.registry
            .snapshot(self.candidate)
            .map_or(true, |record| record.reconciliation_required)
    }

    #[must_use]
    pub const fn candidate(&self) -> PumpCandidateIdentityV1 {
        self.candidate
    }
}

fn publish_ready_with_cas(
    state: &mut CandidateIntegrityRegistryStateV1,
    limits: CandidateIntegrityRegistryLimitsV1,
    cas: CandidateIntegrityCasTokenV1,
    signal: &CandidateIntegritySignalV1,
) -> Result<bool, CandidateIntegrityErrorV1> {
    validate_aliases(state, signal.candidate)?;
    match cas {
        CandidateIntegrityCasTokenV1::Absent => {
            if state.records.contains_key(&signal.candidate) {
                return Ok(false);
            }
            if state.records.len() >= limits.max_candidates {
                return Err(CandidateIntegrityErrorV1::RegistryCapacityExceeded);
            }
            state
                .by_pool
                .insert(signal.candidate.pool_amm_id, signal.candidate);
            state
                .by_mint
                .insert(signal.candidate.mint, signal.candidate);
            state.records.insert(
                signal.candidate,
                CandidateIntegrityRecordV1 {
                    candidate: signal.candidate,
                    outcome: CandidateIntegrityOutcomeV1::Ready,
                    generation: 1,
                    lifecycle_phase: CandidateLifecyclePhaseV1::PreMfs,
                    reconciliation_required: false,
                    witness_quarantined: false,
                    audit_evidence_complete: true,
                    audit_evidence_overflow_count: 0,
                    first_rejected_audit_marker: None,
                    audit_markers: vec![CandidateIntegrityAuditMarkerV1 {
                        generation: 1,
                        outcome: CandidateIntegrityOutcomeV1::Ready,
                        phase_at_observation: CandidateLifecyclePhaseV1::PreMfs,
                        evidence_hash_blake3: signal.evidence_hash_blake3,
                        action: CandidateIntegrityConflictActionV1::ReadyRegistered,
                    }],
                },
            );
            Ok(true)
        }
        CandidateIntegrityCasTokenV1::Generation(expected) => {
            let record = state
                .records
                .get(&signal.candidate)
                .ok_or(CandidateIntegrityErrorV1::CandidateMissing)?;
            if record.generation != expected
                || record.lifecycle_phase != CandidateLifecyclePhaseV1::PreMfs
                || record.outcome != CandidateIntegrityOutcomeV1::Ready
            {
                return Ok(false);
            }
            Ok(false)
        }
    }
}

fn validate_aliases(
    state: &CandidateIntegrityRegistryStateV1,
    candidate: PumpCandidateIdentityV1,
) -> Result<(), CandidateIntegrityErrorV1> {
    if state
        .by_pool
        .get(&candidate.pool_amm_id)
        .is_some_and(|existing| *existing != candidate)
        || state
            .by_mint
            .get(&candidate.mint)
            .is_some_and(|existing| *existing != candidate)
    {
        return Err(CandidateIntegrityErrorV1::CandidateAliasConflict);
    }
    Ok(())
}

fn conflicting_alias_candidates(
    state: &CandidateIntegrityRegistryStateV1,
    candidate: PumpCandidateIdentityV1,
) -> Vec<PumpCandidateIdentityV1> {
    let mut conflicts = HashSet::new();
    if let Some(existing) = state.by_pool.get(&candidate.pool_amm_id) {
        if *existing != candidate {
            conflicts.insert(*existing);
        }
    }
    if let Some(existing) = state.by_mint.get(&candidate.mint) {
        if *existing != candidate {
            conflicts.insert(*existing);
        }
    }
    conflicts.into_iter().collect()
}

fn conflict_action_for_phase(
    record: &mut CandidateIntegrityRecordV1,
) -> CandidateIntegrityConflictActionV1 {
    match record.lifecycle_phase {
        CandidateLifecyclePhaseV1::PreMfs => CandidateIntegrityConflictActionV1::BlockBeforeMfs,
        CandidateLifecyclePhaseV1::MfsMaterialized
        | CandidateLifecyclePhaseV1::EvaluationRunning => {
            CandidateIntegrityConflictActionV1::InterruptEvaluation
        }
        CandidateLifecyclePhaseV1::TerminalReject | CandidateLifecyclePhaseV1::TerminalTimeout => {
            CandidateIntegrityConflictActionV1::TerminalVerdictImmutableAudit
        }
        CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted => {
            CandidateIntegrityConflictActionV1::CancelExecutionBeforeSubmit
        }
        CandidateLifecyclePhaseV1::SubmitStarted => {
            record.reconciliation_required = true;
            CandidateIntegrityConflictActionV1::ReconciliationRequired
        }
        CandidateLifecyclePhaseV1::ConfirmedOpenPosition => {
            record.witness_quarantined = true;
            CandidateIntegrityConflictActionV1::ConfirmedPositionQuarantined
        }
    }
}

fn lookup_record(
    state: &CandidateIntegrityRegistryStateV1,
    candidate: PumpCandidateIdentityV1,
) -> Result<&CandidateIntegrityRecordV1, CandidateIntegrityErrorV1> {
    validate_aliases(state, candidate)?;
    state
        .records
        .get(&candidate)
        .ok_or(CandidateIntegrityErrorV1::CandidateMissing)
}

fn lookup_record_mut(
    state: &mut CandidateIntegrityRegistryStateV1,
    candidate: PumpCandidateIdentityV1,
) -> Result<&mut CandidateIntegrityRecordV1, CandidateIntegrityErrorV1> {
    validate_aliases(state, candidate)?;
    state
        .records
        .get_mut(&candidate)
        .ok_or(CandidateIntegrityErrorV1::CandidateMissing)
}

fn require_ready(record: &CandidateIntegrityRecordV1) -> Result<(), CandidateIntegrityErrorV1> {
    if record.outcome != CandidateIntegrityOutcomeV1::Ready {
        return Err(CandidateIntegrityErrorV1::NotReady(record.outcome));
    }
    Ok(())
}

fn require_generation(
    record: &CandidateIntegrityRecordV1,
    expected: u64,
) -> Result<(), CandidateIntegrityErrorV1> {
    if record.generation != expected {
        return Err(CandidateIntegrityErrorV1::GenerationChanged {
            expected,
            actual: record.generation,
        });
    }
    Ok(())
}

fn append_audit_marker(
    record: &mut CandidateIntegrityRecordV1,
    max_markers: usize,
    marker: CandidateIntegrityAuditMarkerV1,
) {
    if record.audit_markers.len() < max_markers {
        record.audit_markers.push(marker);
    } else {
        record.audit_evidence_complete = false;
        record.audit_evidence_overflow_count =
            record.audit_evidence_overflow_count.saturating_add(1);
        if record.first_rejected_audit_marker.is_none() {
            record.first_rejected_audit_marker = Some(marker);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::{
        CandidateIntegritySignalV1, CanonicalPumpOrderKeyV1, ObservationProvenanceV1,
        ObservationSourceFamilyV1, PumpEconomicCertificationStatusV1, PumpMutationClaimsV1,
        PumpMutationConflictFieldV1, RawPumpMutationLocatorV1,
    };
    use solana_sdk::{pubkey::Pubkey, signature::Signature};

    fn candidate() -> PumpCandidateIdentityV1 {
        PumpCandidateIdentityV1 {
            pool_amm_id: Pubkey::new_unique(),
            mint: Pubkey::new_unique(),
        }
    }

    fn signal(
        candidate: PumpCandidateIdentityV1,
        outcome: CandidateIntegrityOutcomeV1,
        hash_byte: u8,
    ) -> CandidateIntegritySignalV1 {
        CandidateIntegritySignalV1 {
            candidate,
            outcome,
            signature: Some(Signature::new_unique()),
            locator: Some(RawPumpMutationLocatorV1 {
                program_id: Pubkey::new_unique(),
                signature: Signature::new_unique(),
                outer_instruction_index: 0,
                inner_instruction_path: Vec::new(),
                semantic_event_ordinal: 0,
            }),
            conflict_fields: if outcome == CandidateIntegrityOutcomeV1::SourceReconciliationConflict
            {
                vec![PumpMutationConflictFieldV1::TokenAmountUnits]
            } else {
                Vec::new()
            },
            evidence_hash_blake3: [hash_byte; 32],
        }
    }

    fn canonical(
        signature: Signature,
        ordinal: u32,
        candidate: PumpCandidateIdentityV1,
    ) -> StructuralCanonicalPumpMutationV1 {
        let locator = RawPumpMutationLocatorV1 {
            program_id: Pubkey::new_unique(),
            signature,
            outer_instruction_index: ordinal as u16,
            inner_instruction_path: vec![ordinal as u16],
            semantic_event_ordinal: ordinal,
        };
        StructuralCanonicalPumpMutationV1 {
            mutation_family: PumpMutationFamilyV1::Trade,
            locator: locator.clone(),
            order: CanonicalPumpOrderKeyV1 {
                slot: 10,
                tx_index: 0,
                outer_instruction_index: locator.outer_instruction_index,
                inner_instruction_path: locator.inner_instruction_path.clone(),
                semantic_event_ordinal: ordinal,
            },
            claims: PumpMutationClaimsV1 {
                curve: Some(candidate.pool_amm_id),
                mint: Some(candidate.mint),
                success: Some(true),
                ..PumpMutationClaimsV1::default()
            },
            primary_raw_provenance: ObservationProvenanceV1 {
                source_family: ObservationSourceFamilyV1::RawYellowstone,
                source_id: "yellowstone".to_string(),
                provider_id: "primary".to_string(),
                schema_id: "test".to_string(),
                payload_hash_blake3: [ordinal as u8; 32],
                received_at_monotonic_ns: u64::from(ordinal),
            },
            economics_status: PumpEconomicCertificationStatusV1::PendingAnchor,
        }
    }

    fn ready_signal(
        canonical: &StructuralCanonicalPumpMutationV1,
        candidate: PumpCandidateIdentityV1,
    ) -> CandidateIntegritySignalV1 {
        CandidateIntegritySignalV1 {
            candidate,
            outcome: CandidateIntegrityOutcomeV1::Ready,
            signature: Some(canonical.locator.signature),
            locator: Some(canonical.locator.clone()),
            conflict_fields: Vec::new(),
            evidence_hash_blake3: canonical.primary_raw_provenance.payload_hash_blake3,
        }
    }

    #[test]
    fn pr1d_ordering_some_1_ready_is_invisible_until_apply_and_published_once() {
        let registry = CandidateIntegrityRegistry::default();
        let candidate = candidate();
        let signature = Signature::new_unique();
        let canonical = canonical(signature, 0, candidate);
        let receipt = registry
            .stage_canonical_mutation(&canonical)
            .expect("stage receipt");
        registry
            .seal_complete_transaction_inventory(signature, &[ready_signal(&canonical, candidate)])
            .expect("seal inventory");

        assert!(matches!(
            registry.snapshot(candidate),
            Err(CandidateIntegrityErrorV1::CandidateMissing)
        ));
        let first = registry
            .mark_canonical_apply_succeeded(&receipt)
            .expect("ack downstream apply");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].candidate, candidate);
        assert_eq!(
            registry.snapshot(candidate).expect("ready").outcome,
            CandidateIntegrityOutcomeV1::Ready
        );
        assert!(registry
            .mark_canonical_apply_succeeded(&receipt)
            .expect("duplicate ack")
            .is_empty());
    }

    #[test]
    fn pr1d_ordering_final_received_but_not_applied_never_publishes_ready() {
        let registry = CandidateIntegrityRegistry::default();
        let candidate = candidate();
        let signature = Signature::new_unique();
        let first = canonical(signature, 0, candidate);
        let final_mutation = canonical(signature, 1, candidate);
        let first_receipt = registry
            .stage_canonical_mutation(&first)
            .expect("stage first");
        registry
            .mark_canonical_apply_succeeded(&first_receipt)
            .expect("apply first");
        let _final_receipt = registry
            .stage_canonical_mutation(&final_mutation)
            .expect("stage final");
        registry
            .seal_complete_transaction_inventory(
                signature,
                &[ready_signal(&final_mutation, candidate)],
            )
            .expect("seal complete inventory");

        assert!(matches!(
            registry.snapshot(candidate),
            Err(CandidateIntegrityErrorV1::CandidateMissing)
        ));
    }

    #[test]
    fn pr1d_ordering_missing_earlier_locator_cannot_be_satisfied_by_final_locator() {
        let registry = CandidateIntegrityRegistry::default();
        let candidate = candidate();
        let signature = Signature::new_unique();
        let first = canonical(signature, 0, candidate);
        let final_mutation = canonical(signature, 1, candidate);
        let first_receipt = registry
            .stage_canonical_mutation(&first)
            .expect("stage first");
        let final_receipt = registry
            .stage_canonical_mutation(&final_mutation)
            .expect("stage final");
        registry
            .seal_complete_transaction_inventory(
                signature,
                &[ready_signal(&final_mutation, candidate)],
            )
            .expect("seal inventory");

        assert!(registry
            .mark_canonical_apply_succeeded(&final_receipt)
            .expect("apply final")
            .is_empty());
        assert!(matches!(
            registry.snapshot(candidate),
            Err(CandidateIntegrityErrorV1::CandidateMissing)
        ));
        let releases = registry
            .mark_canonical_apply_succeeded(&first_receipt)
            .expect("apply missing locator");
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].candidate, candidate);
    }

    #[test]
    fn pr1d_ordering_candidates_in_one_signature_cannot_cross_satisfy() {
        let registry = CandidateIntegrityRegistry::default();
        let first_candidate = candidate();
        let second_candidate = candidate();
        let signature = Signature::new_unique();
        let first = canonical(signature, 0, first_candidate);
        let second = canonical(signature, 1, second_candidate);
        let first_receipt = registry
            .stage_canonical_mutation(&first)
            .expect("stage first candidate");
        let second_receipt = registry
            .stage_canonical_mutation(&second)
            .expect("stage second candidate");
        registry
            .seal_complete_transaction_inventory(
                signature,
                &[
                    ready_signal(&first, first_candidate),
                    ready_signal(&second, second_candidate),
                ],
            )
            .expect("seal inventory");

        let second_releases = registry
            .mark_canonical_apply_succeeded(&second_receipt)
            .expect("apply second candidate");
        assert_eq!(second_releases.len(), 1);
        assert_eq!(second_releases[0].candidate, second_candidate);
        assert!(matches!(
            registry.snapshot(first_candidate),
            Err(CandidateIntegrityErrorV1::CandidateMissing)
        ));
        let first_releases = registry
            .mark_canonical_apply_succeeded(&first_receipt)
            .expect("apply first candidate");
        assert_eq!(first_releases.len(), 1);
        assert_eq!(first_releases[0].candidate, first_candidate);
    }

    #[test]
    fn pr1d_ordering_final_conflict_or_apply_failure_never_opens_ready() {
        let registry = CandidateIntegrityRegistry::default();
        let candidate_value = candidate();
        let signature = Signature::new_unique();
        let canonical_value = canonical(signature, 0, candidate_value);
        let receipt = registry
            .stage_canonical_mutation(&canonical_value)
            .expect("stage receipt");
        registry
            .record_signal(CandidateIntegritySignalV1 {
                candidate: candidate_value,
                outcome: CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
                signature: Some(signature),
                locator: Some(canonical_value.locator.clone()),
                conflict_fields: vec![PumpMutationConflictFieldV1::TokenAmountUnits],
                evidence_hash_blake3: [9; 32],
            })
            .expect("record derived conflict first");
        registry
            .seal_complete_transaction_inventory(
                signature,
                &[ready_signal(&canonical_value, candidate_value)],
            )
            .expect("seal inventory");
        assert!(registry
            .mark_canonical_apply_succeeded(&receipt)
            .expect("apply canonical despite witness conflict")
            .is_empty());
        assert_eq!(
            registry
                .snapshot(candidate_value)
                .expect("conflict")
                .outcome,
            CandidateIntegrityOutcomeV1::SourceReconciliationConflict
        );

        let other_registry = CandidateIntegrityRegistry::default();
        let other_candidate = candidate();
        let other_signature = Signature::new_unique();
        let other = canonical(other_signature, 0, other_candidate);
        let other_receipt = other_registry
            .stage_canonical_mutation(&other)
            .expect("stage other");
        other_registry
            .seal_complete_transaction_inventory(
                other_signature,
                &[ready_signal(&other, other_candidate)],
            )
            .expect("seal other");
        other_registry
            .fail_canonical_apply(&other_receipt)
            .expect("fail downstream apply");
        assert_eq!(
            other_registry
                .snapshot(other_candidate)
                .expect("failure")
                .outcome,
            CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
        );

        let lagged_registry = CandidateIntegrityRegistry::default();
        let lagged_candidate = candidate();
        let lagged_signature = Signature::new_unique();
        let lagged = canonical(lagged_signature, 0, lagged_candidate);
        lagged_registry
            .stage_canonical_mutation(&lagged)
            .expect("stage lagged receipt");
        lagged_registry
            .seal_complete_transaction_inventory(
                lagged_signature,
                &[ready_signal(&lagged, lagged_candidate)],
            )
            .expect("seal lagged inventory");
        lagged_registry
            .invalidate_pending_canonical_applies()
            .expect("Event Bus lag invalidates pending apply");
        assert_eq!(
            lagged_registry
                .snapshot(lagged_candidate)
                .expect("lag failure")
                .outcome,
            CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
        );
    }

    #[test]
    fn pr1d_ordering_unknown_inventory_applies_structural_mutation_but_never_ready() {
        let registry = CandidateIntegrityRegistry::default();
        let candidate = candidate();
        let canonical = canonical(Signature::new_unique(), 0, candidate);
        let receipt = registry
            .stage_canonical_mutation(&canonical)
            .expect("unknown-inventory mutation still stages");
        assert!(registry
            .mark_canonical_apply_succeeded(&receipt)
            .expect("canonical state apply succeeds")
            .is_empty());
        assert!(matches!(
            registry.snapshot(candidate),
            Err(CandidateIntegrityErrorV1::CandidateMissing)
        ));
    }

    #[test]
    fn pr1d_ordering_final_apply_releases_every_candidate_in_inventory() {
        let registry = CandidateIntegrityRegistry::default();
        let first_candidate = candidate();
        let final_candidate = candidate();
        let signature = Signature::new_unique();
        let first = canonical(signature, 0, first_candidate);
        let final_mutation = canonical(signature, 1, final_candidate);
        let first_receipt = registry
            .stage_canonical_mutation(&first)
            .expect("stage first");
        assert!(registry
            .mark_canonical_apply_succeeded(&first_receipt)
            .expect("apply first before inventory complete")
            .is_empty());
        let final_receipt = registry
            .stage_canonical_mutation(&final_mutation)
            .expect("stage final");
        registry
            .seal_complete_transaction_inventory(
                signature,
                &[
                    ready_signal(&first, first_candidate),
                    ready_signal(&final_mutation, final_candidate),
                ],
            )
            .expect("seal inventory");
        let released = registry
            .mark_canonical_apply_succeeded(&final_receipt)
            .expect("apply final");
        assert_eq!(released.len(), 2);
        assert!(released
            .iter()
            .any(|release| { release.candidate == first_candidate }));
        assert!(released
            .iter()
            .any(|release| { release.candidate == final_candidate }));
    }

    #[test]
    fn fence_capacity_fail_closes_new_candidate_without_evicting_unresolved_receipt() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 1,
            max_audit_markers_per_candidate: 2,
        });
        let pending_candidate = candidate();
        let pending = canonical(Signature::new_unique(), 0, pending_candidate);
        let pending_receipt = registry
            .stage_canonical_mutation(&pending)
            .expect("first unresolved receipt");
        let overflow_candidate = candidate();
        let overflow = canonical(Signature::new_unique(), 0, overflow_candidate);

        assert_eq!(
            registry.stage_canonical_mutation(&overflow),
            Err(CandidateIntegrityErrorV1::RegistryCapacityExceeded)
        );
        assert_eq!(
            registry
                .snapshot(overflow_candidate)
                .expect("typed overflow failure")
                .outcome,
            CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
        );
        let state = registry.lock_state().expect("state");
        assert!(state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .contains_key(&pending_receipt.runtime_key));
    }

    #[test]
    fn ready_capacity_failure_is_atomic_and_cleans_failed_proof() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 1,
            max_audit_markers_per_candidate: 2,
        });
        registry
            .record_signal(signal(candidate(), CandidateIntegrityOutcomeV1::Ready, 1))
            .expect("consume normal record capacity");
        let affected = candidate();
        let canonical = canonical(Signature::new_unique(), 0, affected);
        let receipt = registry
            .stage_canonical_mutation(&canonical)
            .expect("stage affected receipt");
        registry
            .seal_complete_transaction_inventory(
                canonical.locator.signature,
                &[ready_signal(&canonical, affected)],
            )
            .expect("seal affected proof");

        assert_eq!(
            registry.mark_canonical_apply_succeeded(&receipt),
            Err(CandidateIntegrityErrorV1::RegistryCapacityExceeded)
        );
        assert_eq!(
            registry.snapshot(affected).expect("typed failure").outcome,
            CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
        );
        let state = registry.lock_state().expect("state");
        assert!(!state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .contains_key(&receipt.runtime_key));
        assert!(!state
            .canonical_apply_fence
            .proofs_by_signature_candidate
            .contains_key(&(receipt.signature, receipt.candidate)));
    }

    #[test]
    fn seal_capacity_failure_records_exact_candidate_failure_and_resolves_receipt() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 1,
            max_audit_markers_per_candidate: 2,
        });
        let affected = candidate();
        let canonical = canonical(Signature::new_unique(), 0, affected);
        let receipt = registry
            .stage_canonical_mutation(&canonical)
            .expect("stage affected receipt");
        let filler_candidate = candidate();
        let filler_signal = signal(filler_candidate, CandidateIntegrityOutcomeV1::Ready, 3);
        registry
            .lock_state()
            .expect("state")
            .canonical_apply_fence
            .proofs_by_signature_candidate
            .insert(
                (Signature::new_unique(), filler_candidate),
                CandidateApplyProofV1 {
                    integrity_cas: CandidateIntegrityCasTokenV1::Absent,
                    expected_locators: HashSet::new(),
                    ready_signal: filler_signal,
                    invalidated: false,
                    ready_published: false,
                },
            );

        assert_eq!(
            registry.seal_complete_transaction_inventory(
                receipt.signature,
                &[ready_signal(&canonical, affected)],
            ),
            Err(CandidateIntegrityErrorV1::RegistryCapacityExceeded)
        );
        assert_eq!(
            registry.snapshot(affected).expect("typed failure").outcome,
            CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
        );
        assert!(!registry
            .lock_state()
            .expect("state")
            .canonical_apply_fence
            .receipts_by_runtime_key
            .contains_key(&receipt.runtime_key));
    }

    #[test]
    fn failed_ready_delivery_and_terminal_phase_cleanup_resolved_fence_state() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let first_candidate = candidate();
        let first_canonical = canonical(Signature::new_unique(), 0, first_candidate);
        let receipt = registry
            .stage_canonical_mutation(&first_canonical)
            .expect("stage");
        registry
            .seal_complete_transaction_inventory(
                receipt.signature,
                &[ready_signal(&first_canonical, first_candidate)],
            )
            .expect("seal");
        let release = registry
            .mark_canonical_apply_succeeded(&receipt)
            .expect("apply")
            .pop()
            .expect("release");
        registry
            .fail_ready_release(&release)
            .expect("delivery failure");
        assert_eq!(
            registry.snapshot(first_candidate).expect("failed").outcome,
            CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
        );
        let state = registry.lock_state().expect("state");
        assert!(state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .is_empty());
        assert!(state
            .canonical_apply_fence
            .proofs_by_signature_candidate
            .is_empty());
        drop(state);

        let terminal_registry = Arc::new(CandidateIntegrityRegistry::default());
        let terminal_candidate = candidate();
        let terminal = canonical(Signature::new_unique(), 0, terminal_candidate);
        let terminal_receipt = terminal_registry
            .stage_canonical_mutation(&terminal)
            .expect("stage terminal");
        terminal_registry
            .seal_complete_transaction_inventory(
                terminal_receipt.signature,
                &[ready_signal(&terminal, terminal_candidate)],
            )
            .expect("seal terminal");
        terminal_registry
            .mark_canonical_apply_succeeded(&terminal_receipt)
            .expect("apply terminal");
        let guard = terminal_registry
            .evaluation_guard(terminal_candidate)
            .expect("guard");
        guard.mark_mfs_materialized().expect("MFS");
        guard.mark_evaluation_running().expect("evaluation");
        guard
            .publish_terminal(CandidateTerminalTransitionV1::Reject)
            .expect("terminal reject");
        let terminal_state = terminal_registry.lock_state().expect("terminal state");
        assert!(terminal_state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .is_empty());
        assert!(terminal_state
            .canonical_apply_fence
            .proofs_by_signature_candidate
            .is_empty());
    }

    #[test]
    fn conflict_before_mfs_blocks_guard_and_ready_never_heals_it() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let candidate = candidate();
        registry
            .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 1))
            .expect("register ready");
        let result = registry
            .record_signal(signal(
                candidate,
                CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
                2,
            ))
            .expect("record conflict");
        assert_eq!(
            result.action,
            CandidateIntegrityConflictActionV1::BlockBeforeMfs
        );
        registry
            .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 3))
            .expect("late ready is an audited no-op");
        assert!(matches!(
            registry.evaluation_guard(candidate),
            Err(CandidateIntegrityErrorV1::NotReady(
                CandidateIntegrityOutcomeV1::SourceReconciliationConflict
            ))
        ));
    }

    #[test]
    fn conflict_during_evaluation_invalidates_generation_fence() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let candidate = candidate();
        registry
            .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 1))
            .expect("register ready");
        let guard = registry
            .evaluation_guard(candidate)
            .expect("ready evaluation guard");
        guard.mark_mfs_materialized().expect("mark MFS");
        guard.mark_evaluation_running().expect("mark evaluation");
        let result = registry
            .record_signal(signal(
                candidate,
                CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
                2,
            ))
            .expect("record conflict");
        assert_eq!(
            result.action,
            CandidateIntegrityConflictActionV1::InterruptEvaluation
        );
        assert!(matches!(
            guard.check_ready(),
            Err(CandidateIntegrityErrorV1::GenerationChanged { .. })
        ));
    }

    #[test]
    fn terminal_reject_is_immutable_and_only_gets_audit_marker() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let candidate = candidate();
        registry
            .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 1))
            .expect("register ready");
        let guard = registry
            .evaluation_guard(candidate)
            .expect("ready evaluation guard");
        guard.mark_mfs_materialized().expect("mark MFS");
        guard.mark_evaluation_running().expect("mark evaluation");
        guard
            .publish_terminal(CandidateTerminalTransitionV1::Reject)
            .expect("publish reject");
        let result = registry
            .record_signal(signal(
                candidate,
                CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
                2,
            ))
            .expect("record late conflict");
        assert_eq!(
            result.action,
            CandidateIntegrityConflictActionV1::TerminalVerdictImmutableAudit
        );
        assert_eq!(
            result.snapshot.lifecycle_phase,
            CandidateLifecyclePhaseV1::TerminalReject
        );
    }

    #[test]
    fn conflict_before_submit_wins_cas_but_submit_started_is_reconciled() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let first = candidate();
        registry
            .record_signal(signal(first, CandidateIntegrityOutcomeV1::Ready, 1))
            .expect("register ready");
        let evaluation = registry.evaluation_guard(first).expect("guard");
        evaluation.mark_mfs_materialized().expect("MFS");
        evaluation.mark_evaluation_running().expect("evaluation");
        let submit_guard = evaluation
            .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
            .expect("publish BUY")
            .expect("BUY creates submit guard");
        let conflict = registry
            .record_signal(signal(
                first,
                CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
                2,
            ))
            .expect("record conflict");
        assert_eq!(
            conflict.action,
            CandidateIntegrityConflictActionV1::CancelExecutionBeforeSubmit
        );
        assert!(submit_guard.try_begin_submit().is_err());

        let second = candidate();
        registry
            .record_signal(signal(second, CandidateIntegrityOutcomeV1::Ready, 3))
            .expect("register second ready");
        let evaluation = registry.evaluation_guard(second).expect("second guard");
        evaluation.mark_mfs_materialized().expect("MFS");
        evaluation.mark_evaluation_running().expect("evaluation");
        let submit_guard = evaluation
            .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
            .expect("publish BUY")
            .expect("BUY creates submit guard");
        assert_eq!(
            submit_guard.try_begin_submit().expect("begin submit"),
            CandidateSubmitTransitionV1::StartedNow
        );
        let conflict = registry
            .record_signal(signal(
                second,
                CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
                4,
            ))
            .expect("record post-submit conflict");
        assert_eq!(
            conflict.action,
            CandidateIntegrityConflictActionV1::ReconciliationRequired
        );
        submit_guard
            .mark_confirmed()
            .expect("real confirmation remains authoritative");
        let snapshot = registry.snapshot(second).expect("snapshot");
        assert_eq!(
            snapshot.lifecycle_phase,
            CandidateLifecyclePhaseV1::ConfirmedOpenPosition
        );
        assert!(snapshot.reconciliation_required);
    }
}
