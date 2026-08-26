//! Candidate-integrity lifecycle registry for PR1D/PR1E.
//!
//! PR1E activates this bounded registry as the technical admission boundary
//! for new candidates. It remains independent from strategy verdicts and from
//! protective handling of already confirmed positions.

use crate::capture_resilience::CaptureFailureClassV1;
use ghost_core::{
    CandidateIntegrityOutcomeV1, CandidateIntegritySignalV1, PumpCandidateIdentityV1,
    PumpMutationFamilyV1, RawPumpMutationLocatorV1, StructuralCanonicalPumpMutationV1,
};
use serde::{Deserialize, Serialize};
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Condvar, Mutex,
};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pr1AuthorityEpochV1 {
    pub epoch_id: u64,
    pub binary_hash: [u8; 32],
    pub config_hash: [u8; 32],
    pub started_at_unix_ms: u64,
}

impl Pr1AuthorityEpochV1 {
    #[must_use]
    pub fn new(binary_hash: [u8; 32], config_hash: [u8; 32]) -> Self {
        let started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let started_at_unix_ms = started_at.as_millis().min(u128::from(u64::MAX)) as u64;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"ghost.pr1.authority_epoch.v1");
        hasher.update(&binary_hash);
        hasher.update(&config_hash);
        hasher.update(&started_at.as_nanos().to_le_bytes());
        hasher.update(&std::process::id().to_le_bytes());
        let digest = hasher.finalize();
        let mut epoch_bytes = [0u8; 8];
        epoch_bytes.copy_from_slice(&digest.as_bytes()[..8]);
        let epoch_id = u64::from_le_bytes(epoch_bytes).max(1);
        Self {
            epoch_id,
            binary_hash,
            config_hash,
            started_at_unix_ms,
        }
    }

    #[cfg(test)]
    #[must_use]
    pub const fn test_epoch(epoch_id: u64) -> Self {
        Self {
            epoch_id,
            binary_hash: [0; 32],
            config_hash: [0; 32],
            started_at_unix_ms: 0,
        }
    }
}

impl Default for Pr1AuthorityEpochV1 {
    fn default() -> Self {
        Self::new([0; 32], [0; 32])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CandidateIntegrityRegistryLimitsV1 {
    pub max_candidates: usize,
    pub max_audit_markers_per_candidate: usize,
    /// Bounded terminal history retained after an Oracle pool/session has
    /// completed cleanup. The active registry can therefore recycle capacity
    /// without pretending that in-process deduplication is durable forever.
    pub max_terminal_tombstones: usize,
}

impl Default for CandidateIntegrityRegistryLimitsV1 {
    fn default() -> Self {
        Self {
            max_candidates: 100_000,
            max_audit_markers_per_candidate: 32,
            max_terminal_tombstones: 50_000,
        }
    }
}

impl CandidateIntegrityRegistryLimitsV1 {
    fn normalized(self) -> Self {
        Self {
            max_candidates: self.max_candidates.max(1),
            max_audit_markers_per_candidate: self.max_audit_markers_per_candidate.max(1),
            max_terminal_tombstones: self.max_terminal_tombstones.max(1),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CandidateIntegrityTerminalRetirementV1 {
    pub(crate) candidate: PumpCandidateIdentityV1,
}

/// Compact, bounded terminal ownership for a candidate record after its
/// active runtime/session has been removed. A tombstone is never an admission
/// authority: it preserves immutable late-evidence classification while the
/// active maps recover capacity for new candidates.
#[derive(Debug)]
struct TerminalCandidateTombstonesV1 {
    by_candidate: HashMap<PumpCandidateIdentityV1, CandidateIntegrityRecordV1>,
    by_pool: HashMap<solana_sdk::pubkey::Pubkey, PumpCandidateIdentityV1>,
    by_mint: HashMap<solana_sdk::pubkey::Pubkey, PumpCandidateIdentityV1>,
    fifo: VecDeque<PumpCandidateIdentityV1>,
    cap: usize,
    eviction_count: u64,
    first_evicted: Option<CandidateIntegrityRecordV1>,
}

impl TerminalCandidateTombstonesV1 {
    fn new(cap: usize) -> Self {
        Self {
            by_candidate: HashMap::with_capacity(cap.min(4096)),
            by_pool: HashMap::with_capacity(cap.min(4096)),
            by_mint: HashMap::with_capacity(cap.min(4096)),
            fifo: VecDeque::with_capacity(cap.min(4096)),
            cap: cap.max(1),
            eviction_count: 0,
            first_evicted: None,
        }
    }

    fn insert(&mut self, record: CandidateIntegrityRecordV1) -> Option<CandidateIntegrityRecordV1> {
        let candidate = record.candidate;
        if self.by_candidate.contains_key(&candidate) {
            self.by_candidate.insert(candidate, record);
            return None;
        }

        let mut evicted = None;
        while self.by_candidate.len() >= self.cap {
            let Some(oldest) = self.fifo.pop_front() else {
                break;
            };
            if let Some(old_record) = self.by_candidate.remove(&oldest) {
                if self.by_pool.get(&old_record.candidate.pool_amm_id) == Some(&oldest) {
                    self.by_pool.remove(&old_record.candidate.pool_amm_id);
                }
                if self.by_mint.get(&old_record.candidate.mint) == Some(&oldest) {
                    self.by_mint.remove(&old_record.candidate.mint);
                }
                self.eviction_count = self.eviction_count.saturating_add(1);
                if self.first_evicted.is_none() {
                    self.first_evicted = Some(old_record.clone());
                }
                evicted = Some(old_record);
                break;
            }
        }

        self.by_pool.insert(candidate.pool_amm_id, candidate);
        self.by_mint.insert(candidate.mint, candidate);
        self.by_candidate.insert(candidate, record);
        self.fifo.push_back(candidate);
        evicted
    }

    fn get(&self, candidate: PumpCandidateIdentityV1) -> Option<&CandidateIntegrityRecordV1> {
        self.by_candidate.get(&candidate)
    }

    fn get_mut(
        &mut self,
        candidate: PumpCandidateIdentityV1,
    ) -> Option<&mut CandidateIntegrityRecordV1> {
        self.by_candidate.get_mut(&candidate)
    }

    fn alias_conflicts(&self, candidate: PumpCandidateIdentityV1) -> Vec<PumpCandidateIdentityV1> {
        let mut conflicts = HashSet::new();
        if let Some(existing) = self.by_pool.get(&candidate.pool_amm_id) {
            if *existing != candidate {
                conflicts.insert(*existing);
            }
        }
        if let Some(existing) = self.by_mint.get(&candidate.mint) {
            if *existing != candidate {
                conflicts.insert(*existing);
            }
        }
        conflicts.into_iter().collect()
    }

    #[cfg(test)]
    fn retained_count(&self) -> usize {
        self.by_candidate.len()
    }
}

#[derive(Debug)]
struct CandidateIntegrityRegistryStateV1 {
    records: HashMap<PumpCandidateIdentityV1, CandidateIntegrityRecordV1>,
    by_pool: HashMap<solana_sdk::pubkey::Pubkey, PumpCandidateIdentityV1>,
    by_mint: HashMap<solana_sdk::pubkey::Pubkey, PumpCandidateIdentityV1>,
    canonical_apply_fence: CanonicalApplyFenceV1,
    /// Per-candidate linearization fence for terminal Oracle cleanup.
    ///
    /// Once present, no new canonical apply receipt may be staged for this
    /// candidate until Oracle has reclaimed every existing obligation,
    /// retired the candidate, and removed its runtime identity. This prevents
    /// a receipt from appearing between a reclaim snapshot and retirement.
    terminal_cleanup_barriers: HashSet<PumpCandidateIdentityV1>,
    /// In-flight ingest leases that have passed the CandidateIntegrity
    /// boundary but have not yet completed the corresponding
    /// `PumpObservationLedger::observe` plus receipt-stage sequence.
    ///
    /// A terminal cleanup installs its barrier first and then waits only for
    /// these short, synchronous critical sections. This is the
    /// linearization point which prevents the ledger from accepting a late
    /// canonical mutation after its terminal retirement handoff was drained.
    canonical_observation_leases: HashMap<PumpCandidateIdentityV1, usize>,
    terminal_tombstones: TerminalCandidateTombstonesV1,
    /// A bounded handoff to the Seer-owned PumpObservationLedger. Oracle
    /// lifecycle cleanup creates it; Seer's periodic ledger finalizer drains
    /// it synchronously. It is not a retry queue and cannot grant authority.
    terminal_ledger_retirements: VecDeque<CandidateIntegrityTerminalRetirementV1>,
}

impl CandidateIntegrityRegistryStateV1 {
    fn new(max_terminal_tombstones: usize) -> Self {
        Self {
            records: HashMap::new(),
            by_pool: HashMap::new(),
            by_mint: HashMap::new(),
            canonical_apply_fence: CanonicalApplyFenceV1::default(),
            terminal_cleanup_barriers: HashSet::new(),
            canonical_observation_leases: HashMap::new(),
            terminal_tombstones: TerminalCandidateTombstonesV1::new(max_terminal_tombstones),
            terminal_ledger_retirements: VecDeque::with_capacity(max_terminal_tombstones.min(4096)),
        }
    }
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
    pub(crate) authority_epoch_id: u64,
    pub(crate) signature: Signature,
    pub(crate) locator: RawPumpMutationLocatorV1,
    pub(crate) candidate: PumpCandidateIdentityV1,
    pub(crate) evidence_hash_blake3: [u8; 32],
}

#[cfg(test)]
impl CanonicalMutationApplyReceiptV1 {
    pub(crate) fn fixture(
        mutation_family: PumpMutationFamilyV1,
        signature: Signature,
        candidate: PumpCandidateIdentityV1,
        locator: RawPumpMutationLocatorV1,
    ) -> Self {
        Self {
            runtime_key: RuntimeMutationApplyKeyV1 {
                mutation_family,
                signature,
                candidate,
                semantic_event_ordinal: locator.semantic_event_ordinal,
            },
            authority_epoch_id: 1,
            signature,
            locator,
            candidate,
            evidence_hash_blake3: [0xA5; 32],
        }
    }
}

#[derive(Clone, Debug)]
struct CanonicalApplyReceiptStateV1 {
    receipt: CanonicalMutationApplyReceiptV1,
    staged_at: Instant,
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

#[cfg(test)]
#[derive(Clone)]
struct TransitionBeforeCommitHookV1(Arc<dyn Fn() + Send + Sync>);

#[cfg(test)]
impl std::fmt::Debug for TransitionBeforeCommitHookV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TransitionBeforeCommitHookV1(..)")
    }
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
    authority_epoch: Pr1AuthorityEpochV1,
    state: Mutex<CandidateIntegrityRegistryStateV1>,
    /// Wakes terminal cleanup after the last short ingest lease for a
    /// candidate releases. It is paired with `state`, so cleanup never
    /// observes a zero count before the matching lease has completed its
    /// ledger mutation and receipt stage.
    canonical_observation_lease_released: Condvar,
    available: AtomicBool,
    candidate_admission_open: AtomicBool,
    /// Monotonic global admission fence. A guard carries the value observed
    /// when it was issued; closing admission increments this value so an
    /// already-issued MFS/evaluation/submit guard cannot cross the closure.
    ///
    /// This is deliberately separate from a candidate record generation:
    /// global ingest-integrity failure must invalidate every not-yet-started
    /// candidate action without rewriting terminal submit/confirmation state.
    authority_admission_generation: AtomicU64,
    #[cfg(test)]
    transition_before_commit_hook: Mutex<Option<TransitionBeforeCommitHookV1>>,
    #[cfg(test)]
    close_after_state_lock_hook: Mutex<Option<TransitionBeforeCommitHookV1>>,
    #[cfg(test)]
    terminal_cleanup_after_snapshot_hook: Mutex<Option<TransitionBeforeCommitHookV1>>,
    #[cfg(test)]
    canonical_observation_lease_acquired_hook: Mutex<Option<TransitionBeforeCommitHookV1>>,
    #[cfg(test)]
    terminal_cleanup_barrier_installed_hook: Mutex<Option<TransitionBeforeCommitHookV1>>,
}

/// Typed ownership of the short critical region from primary observation
/// intake through `PumpObservationLedger::observe` and the matching canonical
/// receipt stage.
///
/// It carries no evaluation or execution authority. Its sole purpose is to
/// make terminal cleanup and canonical ingest linearizable for one candidate:
/// cleanup first installs its barrier, then waits for already-issued leases;
/// later leases are rejected before they can mutate the ledger.
pub(crate) struct CandidateCanonicalObservationLeaseV1 {
    registry: Arc<CandidateIntegrityRegistry>,
    candidate: PumpCandidateIdentityV1,
}

impl CandidateCanonicalObservationLeaseV1 {
    pub(crate) fn stage_canonical_mutation(
        &self,
        canonical: &StructuralCanonicalPumpMutationV1,
    ) -> Result<CanonicalMutationApplyReceiptV1, CandidateIntegrityErrorV1> {
        self.registry
            .stage_canonical_mutation_with_lease(canonical, self.candidate)
    }
}

impl Drop for CandidateCanonicalObservationLeaseV1 {
    fn drop(&mut self) {
        self.registry
            .release_canonical_observation_lease(self.candidate);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateIntegritySignalResultV1 {
    pub action: CandidateIntegrityConflictActionV1,
    pub snapshot: CandidateIntegrityRecordV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum CandidateIntegrityErrorV1 {
    #[error("candidate integrity registry is unavailable")]
    RegistryUnavailable,
    #[error("candidate integrity registry capacity exceeded")]
    RegistryCapacityExceeded,
    #[error("candidate integrity record is missing")]
    CandidateMissing,
    #[error("candidate terminal retirement has unresolved canonical apply receipts")]
    TerminalRetirementPending,
    #[error("candidate terminal cleanup is in progress; canonical staging is blocked")]
    TerminalCleanupInProgress,
    #[error("candidate identity aliases disagree")]
    CandidateAliasConflict,
    #[error("candidate integrity is not Ready: {0:?}")]
    NotReady(CandidateIntegrityOutcomeV1),
    #[error("candidate integrity generation changed: expected={expected} actual={actual}")]
    GenerationChanged { expected: u64, actual: u64 },
    #[error(
        "candidate admission closed or guard invalidated: expected admission generation={expected} actual={actual}"
    )]
    AdmissionClosed { expected: u64, actual: u64 },
    #[error("candidate lifecycle phase mismatch: expected={expected:?} actual={actual:?}")]
    PhaseMismatch {
        expected: CandidateLifecyclePhaseV1,
        actual: CandidateLifecyclePhaseV1,
    },
}

impl CandidateIntegrityErrorV1 {
    /// Classify the error at the registry boundary.  This deliberately does
    /// not decide whether an already-invalid capture is acceptable; it only
    /// decides whether an individual callsite may close unrelated candidate
    /// admission.
    pub const fn capture_failure_class(&self) -> CaptureFailureClassV1 {
        match self {
            // Registry unavailability is only produced after `mark_unavailable`
            // or a real poisoned state lock.  Both are internal integrity
            // failures, not aliases for a transient external dependency.
            Self::RegistryUnavailable => CaptureFailureClassV1::GlobalRuntimeFatal,
            // Bounded registry/receipt/tombstone capacity means the affected
            // canonical interval cannot be proved complete. Preserve later
            // tape and let finalization reject the segment.
            Self::RegistryCapacityExceeded => CaptureFailureClassV1::CaptureSegmentInvalid,
            // These errors are bound to a candidate, lifecycle transition, or
            // late evidence and must never close unrelated admission.
            Self::CandidateMissing
            | Self::TerminalRetirementPending
            | Self::TerminalCleanupInProgress
            | Self::CandidateAliasConflict
            | Self::NotReady(_)
            | Self::GenerationChanged { .. }
            | Self::AdmissionClosed { .. }
            | Self::PhaseMismatch { .. } => CaptureFailureClassV1::CandidateLocal,
        }
    }
}

impl CandidateIntegrityRegistry {
    fn record_pending_permit_metrics(&self, state: &CandidateIntegrityRegistryStateV1) {
        let now = Instant::now();
        let mut pending = 0usize;
        let mut oldest_age_ms = 0u64;
        for entry in state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .values()
            .filter(|entry| !entry.applied && !entry.failed)
        {
            pending = pending.saturating_add(1);
            oldest_age_ms = oldest_age_ms.max(
                now.saturating_duration_since(entry.staged_at)
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64,
            );
        }
        let epoch = self.authority_epoch.epoch_id.to_string();
        ::metrics::gauge!(
            "pr1_runtime_pending_permits",
            pending as f64,
            "authority_epoch_id" => epoch.clone()
        );
        ::metrics::gauge!(
            "pr1_runtime_oldest_pending_permit_age_ms",
            oldest_age_ms as f64,
            "authority_epoch_id" => epoch
        );
    }

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

    fn has_unresolved_canonical_receipt(
        state: &CandidateIntegrityRegistryStateV1,
        candidate: PumpCandidateIdentityV1,
    ) -> bool {
        state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .values()
            .any(|entry| entry.receipt.candidate == candidate && !entry.applied && !entry.failed)
    }

    /// Resolve the fence ownership for candidates whose canonical receipt has
    /// completed but whose technical integrity outcome is already non-Ready.
    ///
    /// This is deliberately narrower than ordinary Oracle lifecycle cleanup:
    /// it only retires a `PreMfs` record after every receipt for that exact
    /// candidate is resolved. A tombstoned candidate is also cleaned
    /// defensively, so a historical ordering bug cannot leave an applied or
    /// failed receipt consuming bounded fence capacity forever.
    fn cleanup_resolved_non_ready_receipt(
        &self,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<(), CandidateIntegrityErrorV1> {
        let mut state = self.lock_state()?;
        let requires_cleanup = state.terminal_tombstones.get(candidate).is_some()
            || state
                .records
                .get(&candidate)
                .is_some_and(|record| record.outcome != CandidateIntegrityOutcomeV1::Ready);
        if !requires_cleanup {
            return Ok(());
        }
        Self::cleanup_canonical_apply_fence_for_candidate(&mut state, candidate);

        // A terminal Oracle cleanup owns the final record retirement after it
        // has reclaimed all receipt obligations. Keeping that record live
        // while its per-candidate barrier is installed prevents
        // `fail_canonical_apply` from racing the sole cleanup owner and
        // turning a valid terminal reclaim into a later CandidateMissing.
        let terminal_cleanup_owns_retirement = state.terminal_cleanup_barriers.contains(&candidate);
        let retire = !terminal_cleanup_owns_retirement
            && !Self::has_unresolved_canonical_receipt(&state, candidate)
            && state.records.get(&candidate).is_some_and(|record| {
                record.outcome != CandidateIntegrityOutcomeV1::Ready
                    && record.lifecycle_phase == CandidateLifecyclePhaseV1::PreMfs
            });
        if retire {
            match self.retire_resolved_record(&mut state, candidate) {
                Ok(Some(_)) => {
                    ::metrics::counter!(
                        "candidate_integrity_pre_session_terminal_retired_total",
                        1u64,
                        "authority_epoch_id" => self.authority_epoch.epoch_id.to_string()
                    );
                }
                Ok(None) => {
                    return Err(CandidateIntegrityErrorV1::CandidateMissing);
                }
                Err(error) => return Err(error),
            }
        }
        self.record_pending_permit_metrics(&state);
        Ok(())
    }

    /// Move one fully resolved record out of active admission ownership.
    ///
    /// The caller proves that no unresolved receipt remains and that the
    /// lifecycle is safe to retire.  Both ordinary Oracle cleanup and a
    /// terminal pre-session technical failure use this one bounded path, so
    /// neither can turn the active maps into an event-count shutdown budget.
    fn retire_resolved_record(
        &self,
        state: &mut CandidateIntegrityRegistryStateV1,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<Option<CandidateIntegrityRecordV1>, CandidateIntegrityErrorV1> {
        if Self::has_unresolved_canonical_receipt(state, candidate) {
            return Err(CandidateIntegrityErrorV1::TerminalRetirementPending);
        }
        if state.terminal_ledger_retirements.len() >= self.limits.max_terminal_tombstones {
            return Err(CandidateIntegrityErrorV1::RegistryCapacityExceeded);
        }
        let Some(removed) = state.records.remove(&candidate) else {
            return Ok(None);
        };
        if state.by_pool.get(&candidate.pool_amm_id) == Some(&candidate) {
            state.by_pool.remove(&candidate.pool_amm_id);
        }
        if state.by_mint.get(&candidate.mint) == Some(&candidate) {
            state.by_mint.remove(&candidate.mint);
        }
        Self::cleanup_canonical_apply_fence_for_candidate(state, candidate);
        let evicted = state.terminal_tombstones.insert(removed.clone());
        state
            .terminal_ledger_retirements
            .push_back(CandidateIntegrityTerminalRetirementV1 { candidate });
        if evicted.is_some() {
            ::metrics::counter!("candidate_integrity_terminal_tombstone_evicted_total", 1u64);
        }
        ::metrics::gauge!(
            "candidate_integrity_terminal_tombstones",
            state.terminal_tombstones.by_candidate.len() as f64,
            "authority_epoch_id" => self.authority_epoch.epoch_id.to_string()
        );
        Ok(Some(removed))
    }

    #[must_use]
    pub fn new(limits: CandidateIntegrityRegistryLimitsV1) -> Self {
        Self::new_with_epoch(limits, Pr1AuthorityEpochV1::default())
    }

    pub fn new_with_epoch(
        limits: CandidateIntegrityRegistryLimitsV1,
        authority_epoch: Pr1AuthorityEpochV1,
    ) -> Self {
        let limits = limits.normalized();
        Self {
            state: Mutex::new(CandidateIntegrityRegistryStateV1::new(
                limits.max_terminal_tombstones,
            )),
            limits,
            authority_epoch,
            canonical_observation_lease_released: Condvar::new(),
            available: AtomicBool::new(true),
            candidate_admission_open: AtomicBool::new(true),
            authority_admission_generation: AtomicU64::new(1),
            #[cfg(test)]
            transition_before_commit_hook: Mutex::new(None),
            #[cfg(test)]
            close_after_state_lock_hook: Mutex::new(None),
            #[cfg(test)]
            terminal_cleanup_after_snapshot_hook: Mutex::new(None),
            #[cfg(test)]
            canonical_observation_lease_acquired_hook: Mutex::new(None),
            #[cfg(test)]
            terminal_cleanup_barrier_installed_hook: Mutex::new(None),
        }
    }

    /// Acquire the candidate-local lease that must cover a primary
    /// `PumpObservationLedger::observe` and its subsequent canonical receipt
    /// stage. The lease is intentionally acquired before the ledger mutation:
    /// a terminal cleanup that wins first rejects this call without allowing a
    /// new active canonical Ledger record to be created.
    pub(crate) fn acquire_canonical_observation_lease(
        self: &Arc<Self>,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<CandidateCanonicalObservationLeaseV1, CandidateIntegrityErrorV1> {
        self.require_candidate_admission_open()?;
        self.require_available()?;
        let mut state = self.lock_state()?;
        if state.terminal_cleanup_barriers.contains(&candidate)
            || state.terminal_tombstones.get(candidate).is_some()
        {
            return Err(CandidateIntegrityErrorV1::TerminalCleanupInProgress);
        }
        if !state.canonical_observation_leases.contains_key(&candidate)
            && state.canonical_observation_leases.len() >= self.limits.max_candidates
        {
            return Err(CandidateIntegrityErrorV1::RegistryCapacityExceeded);
        }
        *state
            .canonical_observation_leases
            .entry(candidate)
            .or_insert(0) += 1;
        drop(state);
        #[cfg(test)]
        self.invoke_canonical_observation_lease_acquired_hook();
        Ok(CandidateCanonicalObservationLeaseV1 {
            registry: Arc::clone(self),
            candidate,
        })
    }

    fn release_canonical_observation_lease(&self, candidate: PumpCandidateIdentityV1) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.available.store(false, Ordering::Release);
                self.force_close_candidate_admission_without_state(
                    "canonical_observation_lease_mutex_poisoned",
                );
                return;
            }
        };
        match state.canonical_observation_leases.get_mut(&candidate) {
            Some(count) if *count > 1 => *count -= 1,
            Some(_) => {
                state.canonical_observation_leases.remove(&candidate);
                self.canonical_observation_lease_released.notify_all();
            }
            None => {
                self.available.store(false, Ordering::Release);
                drop(state);
                self.close_candidate_admission("canonical_observation_lease_missing_on_release");
            }
        }
    }

    /// Stage a canonical receipt only while the matching pre-ledger lease is
    /// held. The lease is what permits a mutation that began before a terminal
    /// barrier to finish its ledger→receipt sequence; terminal cleanup waits
    /// for that lease before reclaiming and retiring the candidate.
    fn stage_canonical_mutation_with_lease(
        &self,
        canonical: &StructuralCanonicalPumpMutationV1,
        lease_candidate: PumpCandidateIdentityV1,
    ) -> Result<CanonicalMutationApplyReceiptV1, CandidateIntegrityErrorV1> {
        self.stage_canonical_mutation_inner(canonical, Some(lease_candidate))
    }

    #[cfg(test)]
    pub(crate) fn stage_canonical_mutation(
        &self,
        canonical: &StructuralCanonicalPumpMutationV1,
    ) -> Result<CanonicalMutationApplyReceiptV1, CandidateIntegrityErrorV1> {
        self.stage_canonical_mutation_inner(canonical, None)
    }

    fn stage_canonical_mutation_inner(
        &self,
        canonical: &StructuralCanonicalPumpMutationV1,
        lease_candidate: Option<PumpCandidateIdentityV1>,
    ) -> Result<CanonicalMutationApplyReceiptV1, CandidateIntegrityErrorV1> {
        self.require_candidate_admission_open()?;
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
        if let Some(lease_candidate) = lease_candidate {
            if lease_candidate != candidate {
                return Err(CandidateIntegrityErrorV1::CandidateAliasConflict);
            }
        }
        let runtime_key = RuntimeMutationApplyKeyV1 {
            mutation_family: canonical.mutation_family,
            signature: canonical.locator.signature,
            candidate,
            semantic_event_ordinal: canonical.locator.semantic_event_ordinal,
        };
        let receipt = CanonicalMutationApplyReceiptV1 {
            runtime_key: runtime_key.clone(),
            authority_epoch_id: self.authority_epoch.epoch_id,
            signature: canonical.locator.signature,
            locator: canonical.locator.clone(),
            candidate,
            evidence_hash_blake3: canonical.primary_raw_provenance.payload_hash_blake3,
        };
        let mut state = self.lock_state()?;
        let matching_lease_is_active = lease_candidate.is_some_and(|lease_candidate| {
            lease_candidate == candidate
                && state
                    .canonical_observation_leases
                    .get(&candidate)
                    .is_some_and(|count| *count > 0)
        });
        if state.terminal_cleanup_barriers.contains(&candidate) && !matching_lease_is_active {
            return Err(CandidateIntegrityErrorV1::TerminalCleanupInProgress);
        }
        if let Some(existing) = state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .get(&runtime_key)
        {
            if existing.receipt == receipt {
                return Ok(existing.receipt.clone());
            }
            drop(state);
            self.mark_unavailable("canonical_receipt_identity_contradiction");
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
                staged_at: Instant::now(),
                applied: false,
                failed: false,
            },
        );
        self.record_pending_permit_metrics(&state);
        Ok(receipt)
    }

    pub(crate) fn seal_complete_transaction_inventory(
        &self,
        signature: Signature,
        ready_signals: &[CandidateIntegritySignalV1],
    ) -> Result<(), CandidateIntegrityErrorV1> {
        self.require_candidate_admission_open()?;
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
                drop(state);
                self.mark_unavailable("inventory_without_staged_locator");
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
                    drop(state);
                    self.mark_unavailable("inventory_proof_identity_contradiction");
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
            drop(state);
            self.mark_unavailable("ambiguous_canonical_apply_receipt");
            return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
        }
        Ok(first)
    }

    pub(crate) fn fail_canonical_apply(
        &self,
        receipt: &CanonicalMutationApplyReceiptV1,
    ) -> Result<(), CandidateIntegrityErrorV1> {
        self.require_available()?;
        let mut newly_failed = false;
        let identity_contradiction = {
            let mut state = self.lock_state()?;
            let identity_contradiction = {
                let entry = state
                    .canonical_apply_fence
                    .receipts_by_runtime_key
                    .get_mut(&receipt.runtime_key)
                    .ok_or(CandidateIntegrityErrorV1::CandidateMissing)?;
                if entry.receipt != *receipt {
                    true
                } else if entry.applied {
                    return Ok(());
                } else {
                    newly_failed = !entry.failed;
                    entry.failed = true;
                    false
                }
            };
            if !identity_contradiction {
                if let Some(proof) = state
                    .canonical_apply_fence
                    .proofs_by_signature_candidate
                    .get_mut(&(receipt.signature, receipt.candidate))
                {
                    proof.invalidated = true;
                }
                Self::cleanup_canonical_apply_fence_for_candidate(&mut state, receipt.candidate);
                self.record_pending_permit_metrics(&state);
            }
            identity_contradiction
        };
        if identity_contradiction {
            self.mark_unavailable("failed_receipt_identity_contradiction");
            return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
        }
        if newly_failed {
            let _ = self.record_signal(Self::coverage_incomplete_signal(receipt))?;
        }
        if let Err(error) = self.cleanup_resolved_non_ready_receipt(receipt.candidate) {
            self.mark_unavailable("failed_receipt_terminal_cleanup_failed");
            return Err(error);
        }
        Ok(())
    }

    /// Resolve the canonical-apply obligations that still belong to one
    /// terminal Oracle candidate.
    ///
    /// The caller has already proved that the per-pool observation task is
    /// terminal, so no remaining receipt can receive a downstream apply
    /// acknowledgement from that task.  This intentionally delegates every
    /// individual transition to [`Self::fail_canonical_apply`]: it does not
    /// delete fence entries, bypass identity checks, or weaken the bounded
    /// receipt/proof lifecycle.
    pub(crate) fn fail_pending_canonical_applies_for_candidate(
        &self,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<usize, CandidateIntegrityErrorV1> {
        self.require_available()?;
        let pending = {
            let mut state = self.lock_state()?;
            // The barrier linearizes cleanup ahead of new ingest. A lease
            // that was already issued is allowed to finish its short
            // ledger→receipt sequence; waiting here prevents this terminal
            // retirement from draining the Ledger handoff before that mutation
            // is either fenced by a receipt or rejected locally.
            state.terminal_cleanup_barriers.insert(candidate);
            #[cfg(test)]
            {
                drop(state);
                self.invoke_terminal_cleanup_barrier_installed_hook();
                state = self.lock_state()?;
            }
            while state
                .canonical_observation_leases
                .get(&candidate)
                .is_some_and(|count| *count > 0)
            {
                state = match self.canonical_observation_lease_released.wait(state) {
                    Ok(state) => state,
                    Err(_) => {
                        self.available.store(false, Ordering::Release);
                        self.force_close_candidate_admission_without_state(
                            "canonical_observation_lease_wait_mutex_poisoned",
                        );
                        return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
                    }
                };
            }
            state
                .canonical_apply_fence
                .receipts_by_runtime_key
                .values()
                .filter(|entry| {
                    entry.receipt.candidate == candidate && !entry.applied && !entry.failed
                })
                .map(|entry| entry.receipt.clone())
                .collect::<Vec<_>>()
        };

        #[cfg(test)]
        self.invoke_terminal_cleanup_after_snapshot_hook();

        for receipt in &pending {
            self.fail_canonical_apply(receipt)?;
        }
        Ok(pending.len())
    }

    /// Release the terminal-cleanup linearization fence after Oracle has
    /// removed the runtime identity. The fence must never be released while a
    /// receipt remains unresolved.
    pub(crate) fn finish_terminal_candidate_cleanup(
        &self,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<(), CandidateIntegrityErrorV1> {
        self.require_available()?;
        let mut state = self.lock_state()?;
        if Self::has_unresolved_canonical_receipt(&state, candidate) {
            return Err(CandidateIntegrityErrorV1::TerminalRetirementPending);
        }
        state.terminal_cleanup_barriers.remove(&candidate);
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
            self.record_pending_permit_metrics(&state);
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
        // A downstream mutation that races a global integrity closure remains
        // an applied state fact, but it must never turn into a new Ready
        // admission. The closure's generation fence also invalidates every
        // previously issued evaluation/submit guard.
        self.require_candidate_admission_open()?;
        self.require_available()?;
        let mut state = self.lock_state()?;
        let receipt_identity_mismatch = {
            let entry = state
                .canonical_apply_fence
                .receipts_by_runtime_key
                .get_mut(&receipt.runtime_key)
                .ok_or(CandidateIntegrityErrorV1::CandidateMissing)?;
            if entry.receipt != *receipt {
                true
            } else if entry.failed || entry.applied {
                return Ok(Vec::new());
            } else {
                false
            }
        };
        if receipt_identity_mismatch {
            drop(state);
            self.mark_unavailable("applied_receipt_identity_contradiction");
            return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
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
            drop(state);
            self.mark_unavailable("applied_receipt_disappeared");
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
        let resolved_non_ready_or_tombstoned =
            state.terminal_tombstones.get(receipt.candidate).is_some()
                || state.records.get(&receipt.candidate).is_some_and(|record| {
                    record.outcome != CandidateIntegrityOutcomeV1::Ready
                        || matches!(
                            record.lifecycle_phase,
                            CandidateLifecyclePhaseV1::TerminalReject
                                | CandidateLifecyclePhaseV1::TerminalTimeout
                                | CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted
                                | CandidateLifecyclePhaseV1::ConfirmedOpenPosition
                        )
                });
        if resolved_non_ready_or_tombstoned {
            Self::cleanup_canonical_apply_fence_for_candidate(&mut state, receipt.candidate);
        }
        self.record_pending_permit_metrics(&state);
        drop(state);
        if resolved_non_ready_or_tombstoned {
            if let Err(error) = self.cleanup_resolved_non_ready_receipt(receipt.candidate) {
                self.mark_unavailable("applied_receipt_terminal_cleanup_failed");
                return Err(error);
            }
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
                self.mark_unavailable("registry_mutex_poisoned");
                return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
            }
        };
        if let Some(record) = state.terminal_tombstones.get_mut(signal.candidate) {
            // A retired identity remains immutable evidence, never a route
            // back into active candidate admission. Preserve the late signal
            // in the bounded terminal audit history instead of recreating an
            // active record and consuming capacity again.
            append_audit_marker(
                record,
                self.limits.max_audit_markers_per_candidate,
                CandidateIntegrityAuditMarkerV1 {
                    generation: record.generation,
                    outcome: signal.outcome,
                    phase_at_observation: record.lifecycle_phase,
                    evidence_hash_blake3: signal.evidence_hash_blake3,
                    action: CandidateIntegrityConflictActionV1::TerminalVerdictImmutableAudit,
                },
            );
            return Ok(CandidateIntegritySignalResultV1 {
                action: CandidateIntegrityConflictActionV1::TerminalVerdictImmutableAudit,
                snapshot: record.clone(),
            });
        }
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
                drop(state);
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

        // A first non-Ready signal with no unresolved canonical receipt has
        // not opened an Oracle-owned candidate session. Keeping it in the
        // active maps would make malformed/missing wrapper evidence a
        // deterministic capacity-to-shutdown budget. Preserve its immutable
        // technical failure in the same bounded tombstone lane used by
        // ordinary terminal Oracle cleanup instead.
        let pre_session_terminal_failure = inserted
            && signal.outcome != CandidateIntegrityOutcomeV1::Ready
            && !state
                .canonical_apply_fence
                .receipts_by_runtime_key
                .values()
                .any(|entry| {
                    entry.receipt.candidate == signal.candidate && !entry.applied && !entry.failed
                });
        if pre_session_terminal_failure {
            match self.retire_resolved_record(&mut state, signal.candidate) {
                Ok(Some(_)) => {
                    ::metrics::counter!(
                        "candidate_integrity_pre_session_terminal_retired_total",
                        1u64,
                        "authority_epoch_id" => self.authority_epoch.epoch_id.to_string()
                    );
                }
                Ok(None) => {
                    self.available.store(false, Ordering::Release);
                    drop(state);
                    self.close_candidate_admission("pre_session_terminal_record_disappeared");
                    return Err(CandidateIntegrityErrorV1::RegistryUnavailable);
                }
                Err(error) => {
                    drop(state);
                    return Err(error);
                }
            }
        }
        Ok(result)
    }

    pub fn evaluation_guard(
        self: &Arc<Self>,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<CandidateIntegrityEvaluationGuardV1, CandidateIntegrityErrorV1> {
        let admission_generation = self.capture_admission_generation()?;
        self.require_available()?;
        let state = self.lock_state()?;
        self.require_admission_generation(admission_generation)?;
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
            admission_generation,
        })
    }

    pub fn submit_guard(
        self: &Arc<Self>,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<CandidateIntegritySubmitGuardV1, CandidateIntegrityErrorV1> {
        let admission_generation = self.capture_admission_generation()?;
        self.require_available()?;
        let state = self.lock_state()?;
        self.require_admission_generation(admission_generation)?;
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
            admission_generation,
            submit_started: Arc::new(AtomicBool::new(false)),
        })
    }

    #[must_use]
    pub fn snapshot(
        &self,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<CandidateIntegrityRecordV1, CandidateIntegrityErrorV1> {
        let state = self.lock_state()?;
        if let Some(record) = state.records.get(&candidate) {
            return Ok(record.clone());
        }
        state
            .terminal_tombstones
            .get(candidate)
            .cloned()
            .ok_or(CandidateIntegrityErrorV1::CandidateMissing)
    }

    /// Retire a completed runtime candidate from the active admission maps.
    ///
    /// This is called only after the Oracle has removed its session/pool. The
    /// full record is replaced by one bounded terminal tombstone, and an
    /// equally bounded handoff is queued for the Seer-owned observation
    /// ledger. No unresolved canonical receipt is ever silently retired.
    pub(crate) fn retire_terminal_candidate(
        &self,
        candidate: PumpCandidateIdentityV1,
    ) -> Result<bool, CandidateIntegrityErrorV1> {
        self.require_available()?;
        let mut state = self.lock_state()?;
        let has_unresolved_receipt = Self::has_unresolved_canonical_receipt(&state, candidate);
        let Some(record) = state.records.get(&candidate).cloned() else {
            // A receipt may have been staged before inventory completion
            // creates the CandidateIntegrity record. It is still an active
            // proof obligation and must never disappear merely because its
            // candidate has not reached Ready yet.
            if has_unresolved_receipt {
                return Err(CandidateIntegrityErrorV1::TerminalRetirementPending);
            }
            return Ok(false);
        };
        if has_unresolved_receipt {
            return Err(CandidateIntegrityErrorV1::TerminalRetirementPending);
        }
        if record.lifecycle_phase == CandidateLifecyclePhaseV1::SubmitStarted {
            return Err(CandidateIntegrityErrorV1::PhaseMismatch {
                expected: CandidateLifecyclePhaseV1::ConfirmedOpenPosition,
                actual: record.lifecycle_phase,
            });
        }
        self.retire_resolved_record(&mut state, candidate)
            .map(|removed| removed.is_some())
    }

    /// Drain the bounded terminal-retirement control handoff. The caller is
    /// the Seer component's synchronous ledger finalizer; the value carries
    /// no runtime authority by itself. A notice is released only while the
    /// candidate has no unresolved canonical apply receipt. This defensive
    /// revalidation prevents an old/stale handoff from retiring Ledger state
    /// ahead of a receipt that is still in flight.
    pub(crate) fn drain_terminal_ledger_retirements(
        &self,
    ) -> Result<Vec<CandidateIntegrityTerminalRetirementV1>, CandidateIntegrityErrorV1> {
        let mut state = self.lock_state()?;
        let mut ready_for_ledger = Vec::new();
        let mut deferred = VecDeque::with_capacity(state.terminal_ledger_retirements.len());
        while let Some(retirement) = state.terminal_ledger_retirements.pop_front() {
            if Self::has_unresolved_canonical_receipt(&state, retirement.candidate) {
                deferred.push_back(retirement);
                ::metrics::counter!(
                    "candidate_integrity_terminal_retirement_deferred_unresolved_receipt_total",
                    1u64,
                    "authority_epoch_id" => self.authority_epoch.epoch_id.to_string()
                );
            } else {
                ready_for_ledger.push(retirement);
            }
        }
        state.terminal_ledger_retirements = deferred;
        Ok(ready_for_ledger)
    }

    #[cfg(test)]
    pub(crate) fn terminal_tombstone_count(&self) -> Result<usize, CandidateIntegrityErrorV1> {
        Ok(self.lock_state()?.terminal_tombstones.retained_count())
    }

    #[cfg(test)]
    pub(crate) fn active_record_count(&self) -> Result<usize, CandidateIntegrityErrorV1> {
        Ok(self.lock_state()?.records.len())
    }

    #[cfg(test)]
    pub(crate) fn canonical_apply_fence_counts(
        &self,
    ) -> Result<(usize, usize), CandidateIntegrityErrorV1> {
        let state = self.lock_state()?;
        Ok((
            state.canonical_apply_fence.receipts_by_runtime_key.len(),
            state
                .canonical_apply_fence
                .proofs_by_signature_candidate
                .len(),
        ))
    }

    /// Return the current CandidateIntegrity status associated with a
    /// canonical AccountStateCore mutation.
    ///
    /// AccountStateCore authority remains owned independently by the PR1C
    /// AccountObservationArbiter: this query must never suppress raw-primary
    /// updates needed by an already confirmed position. CandidateIntegrity
    /// gates only new-candidate MFS/evaluation/submit admission.
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

    #[must_use]
    pub const fn authority_epoch(&self) -> Pr1AuthorityEpochV1 {
        self.authority_epoch
    }

    #[must_use]
    pub fn candidate_admission_open(&self) -> bool {
        self.candidate_admission_open.load(Ordering::Acquire)
    }

    pub fn close_candidate_admission(&self, reason: &'static str) {
        // `state` is the common linearization lock for global closure and a
        // guard phase transition.  A transition which owns this lock and has
        // rechecked its admission generation commits before a concurrent
        // closure; a closure which owns it first increments the generation
        // before any new transition may commit.  This deliberately permits
        // an already-started submit to retain reconciliation authority while
        // making the close/submit race deterministic.
        let state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.force_close_candidate_admission_without_state(reason);
                return;
            }
        };
        #[cfg(test)]
        self.invoke_close_after_state_lock_hook();
        self.close_candidate_admission_while_state_locked(reason);
        drop(state);
    }

    fn close_candidate_admission_while_state_locked(&self, reason: &'static str) {
        if self.candidate_admission_open.swap(false, Ordering::AcqRel) {
            self.authority_admission_generation
                .fetch_add(1, Ordering::AcqRel);
            crate::oracle_metrics::record_pr1_runtime_candidate_admission_closed();
            ::metrics::gauge!(
                "pr1_runtime_candidate_admission_closed",
                1.0,
                "authority_epoch_id" => self.authority_epoch.epoch_id.to_string(),
                "reason" => reason
            );
        }
    }

    /// Poison handling cannot acquire `state` again.  The unavailable bit is
    /// itself fail-closed; this atomic fallback only records the matching
    /// admission transition and is never a normal guard linearization path.
    fn force_close_candidate_admission_without_state(&self, reason: &'static str) {
        if self.candidate_admission_open.swap(false, Ordering::AcqRel) {
            self.authority_admission_generation
                .fetch_add(1, Ordering::AcqRel);
            crate::oracle_metrics::record_pr1_runtime_candidate_admission_closed();
            ::metrics::gauge!(
                "pr1_runtime_candidate_admission_closed",
                1.0,
                "authority_epoch_id" => self.authority_epoch.epoch_id.to_string(),
                "reason" => reason
            );
        }
    }

    /// Close new-candidate admission and synchronously turn every mutable
    /// candidate record into typed technical integrity evidence.
    ///
    /// This is reserved for proved global registry or canonical-application
    /// corruption. A primary-local coverage gap is instead recorded as a
    /// capture-segment invalidation so later canonical tape can still be
    /// preserved for forensics. Already-started submit/confirmation flows are
    /// not cancelled: they retain reconciliation authority, while confirmed
    /// positions keep their protective lifecycle untouched.
    pub fn close_candidate_admission_with_integrity_invalidation(&self, reason: &'static str) {
        self.close_candidate_admission(reason);
        let admission_generation = self.authority_admission_generation.load(Ordering::Acquire);
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.available.store(false, Ordering::Release);
                return;
            }
        };

        let mut cleanup_candidates = Vec::new();
        for record in state.records.values_mut() {
            let action = match record.lifecycle_phase {
                CandidateLifecyclePhaseV1::PreMfs
                | CandidateLifecyclePhaseV1::MfsMaterialized
                | CandidateLifecyclePhaseV1::EvaluationRunning
                | CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted => {
                    record.generation = record.generation.saturating_add(1);
                    record.outcome = CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete;
                    cleanup_candidates.push(record.candidate);
                    Some(conflict_action_for_phase(record))
                }
                CandidateLifecyclePhaseV1::SubmitStarted => {
                    // The sender has already begun. Preserve that real-world
                    // lifecycle and require reconciliation instead of a false
                    // cancellation.
                    record.reconciliation_required = true;
                    Some(CandidateIntegrityConflictActionV1::ReconciliationRequired)
                }
                CandidateLifecyclePhaseV1::TerminalReject
                | CandidateLifecyclePhaseV1::TerminalTimeout => None,
                CandidateLifecyclePhaseV1::ConfirmedOpenPosition => {
                    // Confirmed position continuity/protective exits are not
                    // candidate admission and must continue after a global
                    // coverage failure.
                    None
                }
            };
            if let Some(action) = action {
                append_audit_marker(
                    record,
                    self.limits.max_audit_markers_per_candidate,
                    CandidateIntegrityAuditMarkerV1 {
                        generation: record.generation,
                        outcome: CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                        phase_at_observation: record.lifecycle_phase,
                        evidence_hash_blake3: global_admission_evidence_hash(
                            reason,
                            record.candidate,
                            admission_generation,
                        ),
                        action,
                    },
                );
            }
        }
        for candidate in cleanup_candidates {
            Self::cleanup_canonical_apply_fence_for_candidate(&mut state, candidate);
        }
        self.record_pending_permit_metrics(&state);
    }

    fn mark_unavailable(&self, reason: &'static str) {
        self.available.store(false, Ordering::Release);
        self.close_candidate_admission(reason);
    }

    fn require_candidate_admission_open(&self) -> Result<(), CandidateIntegrityErrorV1> {
        if self.candidate_admission_open() {
            Ok(())
        } else {
            Err(CandidateIntegrityErrorV1::RegistryUnavailable)
        }
    }

    fn capture_admission_generation(&self) -> Result<u64, CandidateIntegrityErrorV1> {
        self.require_candidate_admission_open()?;
        let generation = self.authority_admission_generation.load(Ordering::Acquire);
        self.require_admission_generation(generation)?;
        Ok(generation)
    }

    fn require_admission_generation(&self, expected: u64) -> Result<(), CandidateIntegrityErrorV1> {
        let actual = self.authority_admission_generation.load(Ordering::Acquire);
        if self.candidate_admission_open() && actual == expected {
            Ok(())
        } else {
            Err(CandidateIntegrityErrorV1::AdmissionClosed { expected, actual })
        }
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
                self.force_close_candidate_admission_without_state("registry_mutex_poisoned");
                Err(CandidateIntegrityErrorV1::RegistryUnavailable)
            }
        }
    }

    fn transition_guard_phase(
        &self,
        candidate: PumpCandidateIdentityV1,
        generation: u64,
        admission_generation: u64,
        expected: CandidateLifecyclePhaseV1,
        next: CandidateLifecyclePhaseV1,
    ) -> Result<(), CandidateIntegrityErrorV1> {
        self.require_available()?;
        self.require_admission_generation(admission_generation)?;
        let mut state = self.lock_state()?;
        self.require_admission_generation(admission_generation)?;
        let record = lookup_record_mut(&mut state, candidate)?;
        require_generation(record, generation)?;
        require_ready(record)?;
        if record.lifecycle_phase != expected {
            return Err(CandidateIntegrityErrorV1::PhaseMismatch {
                expected,
                actual: record.lifecycle_phase,
            });
        }
        #[cfg(test)]
        self.invoke_transition_before_commit_hook();
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

    #[cfg(test)]
    pub(crate) fn set_transition_before_commit_hook(
        &self,
        hook: Option<Arc<dyn Fn() + Send + Sync>>,
    ) {
        *self
            .transition_before_commit_hook
            .lock()
            .expect("test transition hook mutex") = hook.map(TransitionBeforeCommitHookV1);
    }

    #[cfg(test)]
    pub(crate) fn set_close_after_state_lock_hook(
        &self,
        hook: Option<Arc<dyn Fn() + Send + Sync>>,
    ) {
        *self
            .close_after_state_lock_hook
            .lock()
            .expect("test close hook mutex") = hook.map(TransitionBeforeCommitHookV1);
    }

    #[cfg(test)]
    pub(crate) fn set_terminal_cleanup_after_snapshot_hook(
        &self,
        hook: Option<Arc<dyn Fn() + Send + Sync>>,
    ) {
        *self
            .terminal_cleanup_after_snapshot_hook
            .lock()
            .expect("test terminal cleanup hook mutex") = hook.map(TransitionBeforeCommitHookV1);
    }

    #[cfg(test)]
    pub(crate) fn set_canonical_observation_lease_acquired_hook(
        &self,
        hook: Option<Arc<dyn Fn() + Send + Sync>>,
    ) {
        *self
            .canonical_observation_lease_acquired_hook
            .lock()
            .expect("test canonical observation lease hook mutex") =
            hook.map(TransitionBeforeCommitHookV1);
    }

    #[cfg(test)]
    pub(crate) fn set_terminal_cleanup_barrier_installed_hook(
        &self,
        hook: Option<Arc<dyn Fn() + Send + Sync>>,
    ) {
        *self
            .terminal_cleanup_barrier_installed_hook
            .lock()
            .expect("test terminal cleanup barrier hook mutex") =
            hook.map(TransitionBeforeCommitHookV1);
    }

    #[cfg(test)]
    fn invoke_transition_before_commit_hook(&self) {
        let hook = self
            .transition_before_commit_hook
            .lock()
            .expect("test transition hook mutex")
            .clone();
        if let Some(hook) = hook {
            (hook.0)();
        }
    }

    #[cfg(test)]
    fn invoke_close_after_state_lock_hook(&self) {
        let hook = self
            .close_after_state_lock_hook
            .lock()
            .expect("test close hook mutex")
            .clone();
        if let Some(hook) = hook {
            (hook.0)();
        }
    }

    #[cfg(test)]
    fn invoke_terminal_cleanup_after_snapshot_hook(&self) {
        let hook = self
            .terminal_cleanup_after_snapshot_hook
            .lock()
            .expect("test terminal cleanup hook mutex")
            .clone();
        if let Some(hook) = hook {
            (hook.0)();
        }
    }

    #[cfg(test)]
    fn invoke_canonical_observation_lease_acquired_hook(&self) {
        let hook = self
            .canonical_observation_lease_acquired_hook
            .lock()
            .expect("test canonical observation lease hook mutex")
            .clone();
        if let Some(hook) = hook {
            (hook.0)();
        }
    }

    #[cfg(test)]
    fn invoke_terminal_cleanup_barrier_installed_hook(&self) {
        let hook = self
            .terminal_cleanup_barrier_installed_hook
            .lock()
            .expect("test terminal cleanup barrier hook mutex")
            .clone();
        if let Some(hook) = hook {
            (hook.0)();
        }
    }

    fn check_guard(
        &self,
        candidate: PumpCandidateIdentityV1,
        generation: u64,
        admission_generation: u64,
    ) -> Result<(), CandidateIntegrityErrorV1> {
        self.require_available()?;
        self.require_admission_generation(admission_generation)?;
        let state = self.lock_state()?;
        self.require_admission_generation(admission_generation)?;
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
    admission_generation: u64,
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
        self.registry
            .check_guard(self.candidate, self.generation, self.admission_generation)
    }

    pub fn mark_mfs_materialized(&self) -> Result<(), CandidateIntegrityErrorV1> {
        self.registry.transition_guard_phase(
            self.candidate,
            self.generation,
            self.admission_generation,
            CandidateLifecyclePhaseV1::PreMfs,
            CandidateLifecyclePhaseV1::MfsMaterialized,
        )
    }

    pub fn mark_evaluation_running(&self) -> Result<(), CandidateIntegrityErrorV1> {
        self.registry.transition_guard_phase(
            self.candidate,
            self.generation,
            self.admission_generation,
            CandidateLifecyclePhaseV1::MfsMaterialized,
            CandidateLifecyclePhaseV1::EvaluationRunning,
        )
    }

    pub fn reset_pre_mfs(&self) -> Result<(), CandidateIntegrityErrorV1> {
        self.registry.require_available()?;
        self.registry
            .require_admission_generation(self.admission_generation)?;
        let mut state = self.registry.lock_state()?;
        self.registry
            .require_admission_generation(self.admission_generation)?;
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
            self.admission_generation,
            CandidateLifecyclePhaseV1::EvaluationRunning,
            next,
        )?;
        Ok(
            (terminal == CandidateTerminalTransitionV1::BuyNotSubmitted).then(|| {
                CandidateIntegritySubmitGuardV1 {
                    registry: Arc::clone(&self.registry),
                    candidate: self.candidate,
                    generation: self.generation,
                    admission_generation: self.admission_generation,
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
    admission_generation: u64,
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
            self.admission_generation,
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
        if !self.registry.candidate_admission_open() {
            // Global admission closure cannot cancel an already-started
            // sender. It instead requires the existing confirmation path to
            // reconcile the attempt conservatively.
            return true;
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
    if state.terminal_tombstones.get(signal.candidate).is_some() {
        // A late downstream acknowledgement for a retired pool is an audit
        // fact, never a way to resurrect a candidate or publish Ready again.
        return Ok(false);
    }
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
        || state
            .terminal_tombstones
            .by_pool
            .get(&candidate.pool_amm_id)
            .is_some_and(|existing| *existing != candidate)
        || state
            .terminal_tombstones
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
    conflicts.extend(state.terminal_tombstones.alias_conflicts(candidate));
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

fn global_admission_evidence_hash(
    reason: &'static str,
    candidate: PumpCandidateIdentityV1,
    admission_generation: u64,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"ghost.pr1e.global_candidate_admission_closure.v1");
    hasher.update(reason.as_bytes());
    hasher.update(candidate.pool_amm_id.as_ref());
    hasher.update(candidate.mint.as_ref());
    hasher.update(&admission_generation.to_le_bytes());
    *hasher.finalize().as_bytes()
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
    use std::sync::Barrier;

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
    fn fence_capacity_invalidates_only_the_capture_segment_without_closing_admission() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 1,
            max_audit_markers_per_candidate: 2,
            max_terminal_tombstones: 1,
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
        drop(state);
        assert!(registry.candidate_admission_open());
        assert_eq!(
            CandidateIntegrityErrorV1::RegistryCapacityExceeded.capture_failure_class(),
            CaptureFailureClassV1::CaptureSegmentInvalid
        );
    }

    #[test]
    fn alias_conflict_is_candidate_local_not_global_admission_authority() {
        let registry = CandidateIntegrityRegistry::default();

        assert_eq!(
            CandidateIntegrityErrorV1::CandidateAliasConflict.capture_failure_class(),
            CaptureFailureClassV1::CandidateLocal
        );
        assert!(registry.candidate_admission_open());
    }

    #[test]
    fn ready_capacity_failure_is_atomic_and_cleans_failed_proof() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 1,
            max_audit_markers_per_candidate: 2,
            max_terminal_tombstones: 1,
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
        drop(state);
        assert!(registry.candidate_admission_open());
    }

    #[test]
    fn seal_capacity_failure_records_exact_candidate_failure_and_resolves_receipt() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 1,
            max_audit_markers_per_candidate: 2,
            max_terminal_tombstones: 1,
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

    #[test]
    fn conflict_after_confirmation_quarantines_witness_without_rewriting_position() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let candidate = candidate();
        registry
            .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 1))
            .expect("register ready");
        let evaluation = registry.evaluation_guard(candidate).expect("guard");
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
        submit_guard.mark_confirmed().expect("confirm position");

        let late_conflict = registry
            .record_signal(signal(
                candidate,
                CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
                2,
            ))
            .expect("record confirmed-position conflict");
        assert_eq!(
            late_conflict.action,
            CandidateIntegrityConflictActionV1::ConfirmedPositionQuarantined
        );
        assert_eq!(
            late_conflict.snapshot.lifecycle_phase,
            CandidateLifecyclePhaseV1::ConfirmedOpenPosition
        );
        assert!(late_conflict.snapshot.witness_quarantined);
    }

    #[test]
    fn poisoned_registry_cannot_issue_evaluation_or_submit_authority() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let poison_target = Arc::clone(&registry);
        assert!(std::thread::spawn(move || {
            let _guard = poison_target.state.lock().expect("lock before poison");
            panic!("intentional PR1E registry poison");
        })
        .join()
        .is_err());

        assert!(matches!(
            registry.evaluation_guard(candidate()),
            Err(CandidateIntegrityErrorV1::RegistryUnavailable)
        ));
        assert!(!registry.is_available());
        assert!(!registry.candidate_admission_open());
    }

    #[test]
    fn global_admission_close_invalidates_an_issued_evaluation_guard_before_mfs() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let candidate = candidate();
        registry
            .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 1))
            .expect("register Ready");
        let guard = registry
            .evaluation_guard(candidate)
            .expect("issue evaluation guard while admission is open");

        registry.close_candidate_admission("test_global_integrity_failure");

        assert!(matches!(
            guard.check_ready(),
            Err(CandidateIntegrityErrorV1::AdmissionClosed { .. })
        ));
        assert!(matches!(
            guard.mark_mfs_materialized(),
            Err(CandidateIntegrityErrorV1::AdmissionClosed { .. })
        ));
        assert_eq!(
            registry
                .snapshot(candidate)
                .expect("record remains auditable")
                .lifecycle_phase,
            CandidateLifecyclePhaseV1::PreMfs,
            "an invalidated guard cannot begin MFS or invoke Gatekeeper"
        );
    }

    #[test]
    fn global_admission_close_prevents_buy_not_submitted_from_starting_sender() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let candidate = candidate();
        registry
            .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 1))
            .expect("register Ready");
        let evaluation = registry
            .evaluation_guard(candidate)
            .expect("evaluation guard");
        evaluation.mark_mfs_materialized().expect("MFS");
        evaluation
            .mark_evaluation_running()
            .expect("evaluation running");
        let submit_guard = evaluation
            .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
            .expect("BUY terminal")
            .expect("BUY creates submit guard");

        registry.close_candidate_admission("test_global_integrity_failure");

        assert!(matches!(
            submit_guard.try_begin_submit(),
            Err(CandidateIntegrityErrorV1::AdmissionClosed { .. })
        ));
        assert_eq!(
            registry
                .snapshot(candidate)
                .expect("record remains auditable")
                .lifecycle_phase,
            CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted,
            "no sender start is authorized after global admission closure"
        );
    }

    #[test]
    fn global_admission_close_preserves_confirmation_after_submit_started() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let candidate = candidate();
        registry
            .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 1))
            .expect("register Ready");
        let evaluation = registry
            .evaluation_guard(candidate)
            .expect("evaluation guard");
        evaluation.mark_mfs_materialized().expect("MFS");
        evaluation
            .mark_evaluation_running()
            .expect("evaluation running");
        let submit_guard = evaluation
            .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
            .expect("BUY terminal")
            .expect("BUY creates submit guard");
        assert_eq!(
            submit_guard
                .try_begin_submit()
                .expect("submit starts before closure"),
            CandidateSubmitTransitionV1::StartedNow
        );

        registry.close_candidate_admission("test_global_integrity_failure");

        assert!(submit_guard.requires_reconciliation());
        assert_eq!(
            submit_guard
                .try_begin_submit()
                .expect("idempotent submit-start query remains available"),
            CandidateSubmitTransitionV1::AlreadyStarted
        );
        submit_guard
            .mark_confirmed()
            .expect("real confirmation remains authoritative after sender start");
        assert_eq!(
            registry
                .snapshot(candidate)
                .expect("confirmed snapshot")
                .lifecycle_phase,
            CandidateLifecyclePhaseV1::ConfirmedOpenPosition
        );
    }

    #[test]
    fn submit_transition_and_global_close_are_linearized_by_the_registry_state_lock() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let candidate = candidate();
        registry
            .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 1))
            .expect("register Ready");
        let evaluation = registry
            .evaluation_guard(candidate)
            .expect("evaluation guard");
        evaluation.mark_mfs_materialized().expect("MFS");
        evaluation
            .mark_evaluation_running()
            .expect("evaluation running");
        let submit_guard = evaluation
            .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
            .expect("BUY terminal")
            .expect("submit guard");

        let transition_entered = Arc::new(Barrier::new(2));
        let allow_commit = Arc::new(Barrier::new(2));
        let hook_entered = Arc::clone(&transition_entered);
        let hook_release = Arc::clone(&allow_commit);
        registry.set_transition_before_commit_hook(Some(Arc::new(move || {
            hook_entered.wait();
            hook_release.wait();
        })));

        let submit = submit_guard.clone();
        let submit_thread = std::thread::spawn(move || submit.try_begin_submit());
        transition_entered.wait();

        let close_entered = Arc::new(Barrier::new(2));
        let close_barrier = Arc::clone(&close_entered);
        let closing_registry = Arc::clone(&registry);
        let close_thread = std::thread::spawn(move || {
            close_barrier.wait();
            closing_registry.close_candidate_admission("deterministic_submit_close_race");
        });
        close_entered.wait();

        // The submit transition already owns the shared state lock and has
        // passed its generation check. Releasing it commits SubmitStarted;
        // the overlapping close then linearizes afterwards and preserves
        // reconciliation rather than inventing a false cancellation.
        allow_commit.wait();
        assert_eq!(
            submit_thread
                .join()
                .expect("submit thread join")
                .expect("submit wins"),
            CandidateSubmitTransitionV1::StartedNow
        );
        close_thread.join().expect("close thread join");
        registry.set_transition_before_commit_hook(None);

        assert_eq!(
            registry
                .snapshot(candidate)
                .expect("submit-started record")
                .lifecycle_phase,
            CandidateLifecyclePhaseV1::SubmitStarted
        );
        assert!(submit_guard.requires_reconciliation());
        assert!(!registry.candidate_admission_open());
    }

    #[test]
    fn global_close_that_owns_state_lock_first_rejects_a_concurrent_submit_transition() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let candidate = candidate();
        registry
            .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 1))
            .expect("register Ready");
        let evaluation = registry
            .evaluation_guard(candidate)
            .expect("evaluation guard");
        evaluation.mark_mfs_materialized().expect("MFS");
        evaluation
            .mark_evaluation_running()
            .expect("evaluation running");
        let submit_guard = evaluation
            .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
            .expect("BUY terminal")
            .expect("submit guard");

        let close_holds_state = Arc::new(Barrier::new(2));
        let release_close = Arc::new(Barrier::new(2));
        let entered = Arc::clone(&close_holds_state);
        let release = Arc::clone(&release_close);
        registry.set_close_after_state_lock_hook(Some(Arc::new(move || {
            entered.wait();
            release.wait();
        })));

        let closing_registry = Arc::clone(&registry);
        let close_thread = std::thread::spawn(move || {
            closing_registry.close_candidate_admission("deterministic_close_wins_race");
        });
        close_holds_state.wait();

        let submit_started = Arc::new(Barrier::new(2));
        let submit_entered = Arc::clone(&submit_started);
        let submit = submit_guard.clone();
        let submit_thread = std::thread::spawn(move || {
            submit_entered.wait();
            submit.try_begin_submit()
        });
        submit_started.wait();

        // The close already owns the common state lock.  It increments the
        // admission generation before the waiting submit transition can make
        // its second guarded check, so close deterministically wins.
        release_close.wait();
        close_thread.join().expect("close thread join");
        assert!(matches!(
            submit_thread.join().expect("submit thread join"),
            Err(CandidateIntegrityErrorV1::AdmissionClosed { .. })
        ));
        registry.set_close_after_state_lock_hook(None);
        assert!(!registry.candidate_admission_open());
        assert_eq!(
            registry
                .snapshot(candidate)
                .expect("closed candidate snapshot")
                .lifecycle_phase,
            CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted
        );
    }

    #[test]
    fn terminal_retirement_reclaims_active_capacity_without_resurrecting_ready() {
        let registry = Arc::new(CandidateIntegrityRegistry::new(
            CandidateIntegrityRegistryLimitsV1 {
                max_candidates: 1,
                max_audit_markers_per_candidate: 2,
                max_terminal_tombstones: 2,
            },
        ));
        let retired = candidate();
        registry
            .record_signal(signal(retired, CandidateIntegrityOutcomeV1::Ready, 1))
            .expect("active candidate");

        assert!(registry
            .retire_terminal_candidate(retired)
            .expect("terminal pool has no unresolved receipt"));
        assert_eq!(
            registry
                .terminal_tombstone_count()
                .expect("bounded tombstone snapshot"),
            1
        );
        assert!(matches!(
            registry.evaluation_guard(retired),
            Err(CandidateIntegrityErrorV1::CandidateMissing)
        ));
        let late = registry
            .record_signal(signal(
                retired,
                CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
                2,
            ))
            .expect("late retired evidence remains auditable");
        assert_eq!(
            late.action,
            CandidateIntegrityConflictActionV1::TerminalVerdictImmutableAudit
        );

        let next = candidate();
        registry
            .record_signal(signal(next, CandidateIntegrityOutcomeV1::Ready, 3))
            .expect("reclaimed active capacity admits the next candidate");
        assert!(registry.evaluation_guard(next).is_ok());
        assert!(registry.candidate_admission_open());

        let retirements = registry
            .drain_terminal_ledger_retirements()
            .expect("bounded handoff");
        assert_eq!(retirements.len(), 1);
        assert_eq!(retirements[0].candidate, retired);
        assert!(registry
            .drain_terminal_ledger_retirements()
            .expect("handoff drain is idempotent")
            .is_empty());
    }

    #[test]
    fn terminal_retirement_never_discards_an_unresolved_apply_receipt() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 1,
            max_audit_markers_per_candidate: 2,
            max_terminal_tombstones: 1,
        });
        let candidate = candidate();
        let canonical = canonical(Signature::new_unique(), 0, candidate);
        let receipt = registry
            .stage_canonical_mutation(&canonical)
            .expect("stage unresolved apply");

        assert_eq!(
            registry.retire_terminal_candidate(candidate),
            Err(CandidateIntegrityErrorV1::TerminalRetirementPending)
        );
        let state = registry.lock_state().expect("registry state");
        assert!(state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .contains_key(&receipt.runtime_key));
        assert_eq!(state.terminal_tombstones.retained_count(), 0);
    }

    #[test]
    fn terminal_cleanup_barrier_blocks_receipt_staging_between_reclaim_and_retirement() {
        let registry = Arc::new(CandidateIntegrityRegistry::default());
        let candidate = candidate();
        registry
            .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 0x11))
            .expect("candidate must be admitted before terminal cleanup");

        let owned = canonical(Signature::new_unique(), 0, candidate);
        let _owned_receipt = registry
            .stage_canonical_mutation(&owned)
            .expect("receipt owned by terminal task");

        let snapshot_taken = Arc::new(Barrier::new(2));
        let release_reclaim = Arc::new(Barrier::new(2));
        let snapshot_hook = Arc::clone(&snapshot_taken);
        let release_hook = Arc::clone(&release_reclaim);
        registry.set_terminal_cleanup_after_snapshot_hook(Some(Arc::new(move || {
            snapshot_hook.wait();
            release_hook.wait();
        })));

        let reclaim_registry = Arc::clone(&registry);
        let reclaim_thread = std::thread::spawn(move || {
            reclaim_registry.fail_pending_canonical_applies_for_candidate(candidate)
        });
        snapshot_taken.wait();

        let late = canonical(Signature::new_unique(), 1, candidate);
        assert_eq!(
            registry.stage_canonical_mutation(&late),
            Err(CandidateIntegrityErrorV1::TerminalCleanupInProgress),
            "a receipt must not enter after the reclaim snapshot and before retirement"
        );
        release_reclaim.wait();
        assert_eq!(
            reclaim_thread
                .join()
                .expect("terminal reclaim thread join")
                .expect("terminal reclaim under barrier"),
            1
        );
        registry.set_terminal_cleanup_after_snapshot_hook(None);
        assert!(
            registry.retire_terminal_candidate(candidate).is_ok(),
            "all obligations were reclaimed before retirement"
        );
        registry
            .finish_terminal_candidate_cleanup(candidate)
            .expect("barrier releases only after terminal cleanup");
        assert_eq!(
            registry
                .canonical_apply_fence_counts()
                .expect("fence counts"),
            (0, 0),
            "terminal cleanup leaves no unresolved receipt or proof"
        );
        assert!(registry.candidate_admission_open());
    }

    #[test]
    fn terminal_tombstone_fifo_reclaims_active_registry_capacity_past_retention_cap() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 1,
            max_audit_markers_per_candidate: 2,
            max_terminal_tombstones: 2,
        });

        for sequence in 0..4_u8 {
            let terminal = candidate();
            registry
                .record_signal(signal(
                    terminal,
                    CandidateIntegrityOutcomeV1::Ready,
                    sequence,
                ))
                .expect("active capacity is reclaimed before the next candidate arrives");
            assert!(registry
                .retire_terminal_candidate(terminal)
                .expect("resolved candidate retires into bounded terminal lane"));
            assert_eq!(
                registry
                    .drain_terminal_ledger_retirements()
                    .expect("test drains the cross-owner handoff")
                    .len(),
                1
            );
        }

        let state = registry.lock_state().expect("registry state");
        assert_eq!(state.records.len(), 0);
        assert_eq!(state.terminal_tombstones.retained_count(), 2);
        assert_eq!(state.terminal_tombstones.eviction_count, 2);
        assert!(state.terminal_tombstones.first_evicted.is_some());
    }

    #[test]
    fn pre_session_technical_failures_retire_into_bounded_terminal_history() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 1,
            max_audit_markers_per_candidate: 2,
            max_terminal_tombstones: 2,
        });
        let first = candidate();
        let first_result = registry
            .record_signal(signal(
                first,
                CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                1,
            ))
            .expect("pre-session failure is retained as terminal evidence");
        assert_eq!(
            first_result.snapshot.outcome,
            CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
        );
        {
            let state = registry.lock_state().expect("registry state");
            assert!(
                state.records.is_empty(),
                "pre-session failure is not active capacity"
            );
            assert_eq!(state.terminal_tombstones.retained_count(), 1);
        }
        assert_eq!(
            registry
                .drain_terminal_ledger_retirements()
                .expect("bounded ledger handoff")
                .len(),
            1
        );

        let second = candidate();
        registry
            .record_signal(signal(
                second,
                CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                2,
            ))
            .expect("next pre-session failure must not exhaust active capacity");
        assert!(registry.candidate_admission_open());
        assert_eq!(
            registry
                .snapshot(first)
                .expect("first terminal audit")
                .outcome,
            CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
        );
    }

    #[test]
    fn pre_session_failure_with_unresolved_receipt_remains_active_until_receipt_resolves() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 2,
            max_audit_markers_per_candidate: 2,
            max_terminal_tombstones: 2,
        });
        let candidate = candidate();
        let receipt = registry
            .stage_canonical_mutation(&canonical(Signature::new_unique(), 0, candidate))
            .expect("stage unresolved receipt");
        registry
            .record_signal(signal(
                candidate,
                CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                3,
            ))
            .expect("technical signal remains active while receipt is unresolved");
        let state = registry.lock_state().expect("registry state");
        assert!(state.records.contains_key(&candidate));
        assert!(state
            .canonical_apply_fence
            .receipts_by_runtime_key
            .contains_key(&receipt.runtime_key));
    }

    #[test]
    fn resolved_non_ready_canonical_apply_retires_and_reclaims_the_entire_fence() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 2,
            max_audit_markers_per_candidate: 2,
            max_terminal_tombstones: 2,
        });
        let candidate = candidate();
        let canonical = canonical(Signature::new_unique(), 0, candidate);
        let receipt = registry
            .stage_canonical_mutation(&canonical)
            .expect("canonical receipt staged before non-Ready evidence");
        registry
            .record_signal(signal(
                candidate,
                CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                1,
            ))
            .expect("non-Ready candidate remains active while receipt is unresolved");

        assert_eq!(registry.active_record_count().expect("active count"), 1);
        assert_eq!(
            registry
                .canonical_apply_fence_counts()
                .expect("fence counts"),
            (1, 0)
        );

        assert!(registry
            .mark_canonical_apply_succeeded(&receipt)
            .expect("downstream state mutation is acknowledged")
            .is_empty());

        assert_eq!(registry.active_record_count().expect("active count"), 0);
        assert_eq!(
            registry
                .terminal_tombstone_count()
                .expect("terminal evidence count"),
            1
        );
        assert_eq!(
            registry
                .canonical_apply_fence_counts()
                .expect("resolved fence is reclaimed"),
            (0, 0)
        );
        assert!(registry.candidate_admission_open());
        assert_eq!(
            registry
                .drain_terminal_ledger_retirements()
                .expect("resolved terminal handoff"),
            vec![CandidateIntegrityTerminalRetirementV1 { candidate }]
        );
    }

    #[test]
    fn failed_non_ready_canonical_apply_retires_and_reclaims_the_entire_fence() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 2,
            max_audit_markers_per_candidate: 2,
            max_terminal_tombstones: 2,
        });
        let candidate = candidate();
        let receipt = registry
            .stage_canonical_mutation(&canonical(Signature::new_unique(), 0, candidate))
            .expect("canonical receipt staged");
        registry
            .record_signal(signal(
                candidate,
                CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                1,
            ))
            .expect("non-Ready evidence recorded");

        registry
            .fail_canonical_apply(&receipt)
            .expect("failed downstream apply resolves its receipt");

        assert_eq!(registry.active_record_count().expect("active count"), 0);
        assert_eq!(
            registry
                .canonical_apply_fence_counts()
                .expect("failed fence is reclaimed"),
            (0, 0)
        );
        assert_eq!(
            registry
                .terminal_tombstone_count()
                .expect("terminal evidence count"),
            1
        );
        assert!(registry.candidate_admission_open());
    }

    #[test]
    fn terminal_ledger_handoff_waits_for_a_late_staged_receipt_to_resolve() {
        let registry = CandidateIntegrityRegistry::new(CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 2,
            max_audit_markers_per_candidate: 2,
            max_terminal_tombstones: 2,
        });
        let candidate = candidate();
        registry
            .record_signal(signal(
                candidate,
                CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
                1,
            ))
            .expect("simulate historical pre-receipt terminal evidence");
        let receipt = registry
            .stage_canonical_mutation(&canonical(Signature::new_unique(), 0, candidate))
            .expect("late staged receipt");

        assert!(registry
            .drain_terminal_ledger_retirements()
            .expect("unresolved handoff is retained")
            .is_empty());
        assert_eq!(
            registry
                .canonical_apply_fence_counts()
                .expect("late receipt remains owned"),
            (1, 0)
        );

        registry
            .fail_canonical_apply(&receipt)
            .expect("late receipt failure resolves ownership");
        assert_eq!(
            registry
                .canonical_apply_fence_counts()
                .expect("resolved late receipt is reclaimed"),
            (0, 0)
        );
        assert_eq!(
            registry
                .drain_terminal_ledger_retirements()
                .expect("handoff becomes eligible after resolution"),
            vec![CandidateIntegrityTerminalRetirementV1 { candidate }]
        );
    }
}
