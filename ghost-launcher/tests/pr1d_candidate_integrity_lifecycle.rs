//! Shadow-only PR1D lifecycle matrix.
//!
//! These tests validate the actions that CandidateIntegrity records as
//! `would_*` evidence. They do not describe active MFS, Gatekeeper, submit,
//! confirmation, capacity, or post-buy enforcement in PR1D.

use ghost_core::{
    CandidateIntegrityOutcomeV1, CandidateIntegritySignalV1, PumpCandidateIdentityV1,
    PumpMutationConflictFieldV1, RawPumpMutationLocatorV1,
};
use ghost_launcher::candidate_integrity::{
    CandidateIntegrityConflictActionV1, CandidateIntegrityErrorV1,
    CandidateIntegrityEvaluationGuardV1, CandidateIntegrityRegistry,
    CandidateIntegrityRegistryLimitsV1, CandidateIntegritySubmitGuardV1, CandidateLifecyclePhaseV1,
    CandidateSubmitTransitionV1, CandidateTerminalTransitionV1,
};
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use std::sync::{Arc, Barrier};
use std::thread;

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
            outer_instruction_index: 1,
            inner_instruction_path: vec![0],
            semantic_event_ordinal: u32::from(hash_byte),
        }),
        conflict_fields: if outcome == CandidateIntegrityOutcomeV1::SourceReconciliationConflict {
            vec![PumpMutationConflictFieldV1::TokenAmountUnits]
        } else {
            Vec::new()
        },
        evidence_hash_blake3: [hash_byte; 32],
    }
}

fn ready_registry_with_limits(
    candidate: PumpCandidateIdentityV1,
    limits: CandidateIntegrityRegistryLimitsV1,
) -> Arc<CandidateIntegrityRegistry> {
    let registry = Arc::new(CandidateIntegrityRegistry::new(limits));
    let registered = registry
        .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 1))
        .expect("ready candidate must be registered");
    assert_eq!(
        registered.action,
        CandidateIntegrityConflictActionV1::ReadyRegistered
    );
    registry
}

fn ready_registry(candidate: PumpCandidateIdentityV1) -> Arc<CandidateIntegrityRegistry> {
    ready_registry_with_limits(candidate, CandidateIntegrityRegistryLimitsV1::default())
}

fn evaluation_running(
    registry: &Arc<CandidateIntegrityRegistry>,
    candidate: PumpCandidateIdentityV1,
) -> CandidateIntegrityEvaluationGuardV1 {
    let guard = registry
        .evaluation_guard(candidate)
        .expect("Ready candidate must receive an evaluation guard");
    guard
        .mark_mfs_materialized()
        .expect("Ready generation must cross the MFS fence");
    guard
        .mark_evaluation_running()
        .expect("the same Ready generation must enter evaluation");
    guard
}

fn buy_not_submitted(
    registry: &Arc<CandidateIntegrityRegistry>,
    candidate: PumpCandidateIdentityV1,
) -> CandidateIntegritySubmitGuardV1 {
    evaluation_running(registry, candidate)
        .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
        .expect("BUY transition must be published")
        .expect("BUY-not-submitted must return the only submit permit")
}

fn confirmed_position(
    registry: &Arc<CandidateIntegrityRegistry>,
    candidate: PumpCandidateIdentityV1,
) -> CandidateIntegritySubmitGuardV1 {
    let submit_guard = buy_not_submitted(registry, candidate);
    assert_eq!(
        submit_guard
            .try_begin_submit()
            .expect("Ready BUY must start submit"),
        CandidateSubmitTransitionV1::StartedNow
    );
    submit_guard
        .mark_confirmed()
        .expect("real confirmation must establish the position");
    submit_guard
}

#[test]
fn shadow_pre_mfs_integrity_failure_computes_would_block() {
    let candidate = candidate();
    let registry = ready_registry(candidate);

    let blocked = registry
        .record_signal(signal(
            candidate,
            CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete,
            2,
        ))
        .expect("technical failure must be recorded");

    assert_eq!(
        blocked.action,
        CandidateIntegrityConflictActionV1::BlockBeforeMfs
    );
    assert_eq!(
        blocked.snapshot.lifecycle_phase,
        CandidateLifecyclePhaseV1::PreMfs
    );
    assert_eq!(
        blocked.snapshot.outcome,
        CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
    );
    assert!(matches!(
        registry.evaluation_guard(candidate),
        Err(CandidateIntegrityErrorV1::NotReady(
            CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
        ))
    ));

    let late_ready = registry
        .record_signal(signal(candidate, CandidateIntegrityOutcomeV1::Ready, 3))
        .expect("late Ready is an audited no-op");
    assert_eq!(
        late_ready.action,
        CandidateIntegrityConflictActionV1::ExistingFailurePreserved
    );
    assert_eq!(
        late_ready.snapshot.lifecycle_phase,
        CandidateLifecyclePhaseV1::PreMfs
    );
}

#[test]
fn shadow_mfs_and_evaluation_compute_would_abort_for_late_conflicts() {
    for expected_phase in [
        CandidateLifecyclePhaseV1::MfsMaterialized,
        CandidateLifecyclePhaseV1::EvaluationRunning,
    ] {
        let candidate = candidate();
        let registry = ready_registry(candidate);
        let guard = registry
            .evaluation_guard(candidate)
            .expect("Ready candidate must receive a guard");
        guard
            .mark_mfs_materialized()
            .expect("MFS transition must succeed");
        if expected_phase == CandidateLifecyclePhaseV1::EvaluationRunning {
            guard
                .mark_evaluation_running()
                .expect("evaluation transition must succeed");
        }

        let interrupted = registry
            .record_signal(signal(
                candidate,
                CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
                2,
            ))
            .expect("late conflict must be recorded");

        assert_eq!(
            interrupted.action,
            CandidateIntegrityConflictActionV1::InterruptEvaluation
        );
        assert_eq!(interrupted.snapshot.lifecycle_phase, expected_phase);
        assert!(matches!(
            guard.check_ready(),
            Err(CandidateIntegrityErrorV1::GenerationChanged { .. })
        ));
    }
}

#[test]
fn terminal_reject_and_timeout_are_immutable_audit_boundaries() {
    for (terminal, expected_phase) in [
        (
            CandidateTerminalTransitionV1::Reject,
            CandidateLifecyclePhaseV1::TerminalReject,
        ),
        (
            CandidateTerminalTransitionV1::Timeout,
            CandidateLifecyclePhaseV1::TerminalTimeout,
        ),
    ] {
        let candidate = candidate();
        let registry = ready_registry(candidate);
        let terminal_guard = evaluation_running(&registry, candidate);
        assert!(
            terminal_guard
                .publish_terminal(terminal)
                .expect("terminal transition must succeed")
                .is_none(),
            "Reject and Timeout must never mint a submit permit"
        );

        let late_conflict = registry
            .record_signal(signal(
                candidate,
                CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
                2,
            ))
            .expect("late conflict must remain auditable");

        assert_eq!(
            late_conflict.action,
            CandidateIntegrityConflictActionV1::TerminalVerdictImmutableAudit
        );
        assert_eq!(late_conflict.snapshot.lifecycle_phase, expected_phase);
        assert!(registry.submit_guard(candidate).is_err());
    }
}

#[test]
fn shadow_conflict_before_submit_computes_would_cancel() {
    let candidate = candidate();
    let registry = ready_registry(candidate);
    let submit_guard = buy_not_submitted(&registry, candidate);

    let conflict = registry
        .record_signal(signal(
            candidate,
            CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
            2,
        ))
        .expect("pre-submit conflict must be recorded");

    assert_eq!(
        conflict.action,
        CandidateIntegrityConflictActionV1::CancelExecutionBeforeSubmit
    );
    assert_eq!(
        conflict.snapshot.lifecycle_phase,
        CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted
    );
    assert!(!conflict.snapshot.reconciliation_required);
    assert!(matches!(
        submit_guard.try_begin_submit(),
        Err(CandidateIntegrityErrorV1::GenerationChanged { .. })
            | Err(CandidateIntegrityErrorV1::NotReady(_))
    ));
    assert_eq!(
        registry
            .snapshot(candidate)
            .expect("candidate snapshot")
            .lifecycle_phase,
        CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted
    );
}

#[test]
fn shadow_conflict_after_submit_computes_would_reconcile() {
    let candidate = candidate();
    let registry = ready_registry(candidate);
    let submit_guard = buy_not_submitted(&registry, candidate);
    assert_eq!(
        submit_guard
            .try_begin_submit()
            .expect("submit must start exactly once"),
        CandidateSubmitTransitionV1::StartedNow
    );

    let conflict = registry
        .record_signal(signal(
            candidate,
            CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
            2,
        ))
        .expect("post-submit conflict must be recorded");
    assert_eq!(
        conflict.action,
        CandidateIntegrityConflictActionV1::ReconciliationRequired
    );
    assert_eq!(
        conflict.snapshot.lifecycle_phase,
        CandidateLifecyclePhaseV1::SubmitStarted
    );
    assert!(conflict.snapshot.reconciliation_required);

    submit_guard
        .mark_confirmed()
        .expect("only the explicit confirmation transition establishes a position");
    let confirmed = registry.snapshot(candidate).expect("confirmed snapshot");
    assert_eq!(
        confirmed.lifecycle_phase,
        CandidateLifecyclePhaseV1::ConfirmedOpenPosition
    );
    assert!(confirmed.reconciliation_required);
}

#[test]
fn shadow_conflict_after_confirmation_computes_would_quarantine() {
    let candidate = candidate();
    let registry = ready_registry(candidate);
    let _submit_guard = confirmed_position(&registry, candidate);

    let conflict = registry
        .record_signal(signal(
            candidate,
            CandidateIntegrityOutcomeV1::AccountProviderConflict,
            2,
        ))
        .expect("confirmed-position conflict must be recorded");

    assert_eq!(
        conflict.action,
        CandidateIntegrityConflictActionV1::ConfirmedPositionQuarantined
    );
    assert_eq!(
        conflict.snapshot.lifecycle_phase,
        CandidateLifecyclePhaseV1::ConfirmedOpenPosition
    );
    assert!(conflict.snapshot.witness_quarantined);
    assert!(!conflict.snapshot.reconciliation_required);
}

#[test]
fn shadow_conflict_vs_submit_race_has_only_the_two_recorded_outcomes() {
    const RACE_ATTEMPTS: u8 = 128;

    for attempt in 0..RACE_ATTEMPTS {
        let candidate = candidate();
        let registry = ready_registry(candidate);
        let submit_guard = buy_not_submitted(&registry, candidate);
        let start = Arc::new(Barrier::new(3));

        let submit_start = Arc::clone(&start);
        let raced_submit_guard = submit_guard.clone();
        let submit_thread = thread::spawn(move || {
            submit_start.wait();
            raced_submit_guard.try_begin_submit()
        });

        let conflict_start = Arc::clone(&start);
        let conflict_registry = Arc::clone(&registry);
        let conflict_thread = thread::spawn(move || {
            conflict_start.wait();
            conflict_registry.record_signal(signal(
                candidate,
                CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
                attempt.wrapping_add(2),
            ))
        });

        start.wait();
        let submit_result = submit_thread.join().expect("submit thread must not panic");
        let conflict_result = conflict_thread
            .join()
            .expect("conflict thread must not panic")
            .expect("conflict signal must be recorded");

        match submit_result {
            Ok(CandidateSubmitTransitionV1::StartedNow) => {
                assert_eq!(
                    conflict_result.action,
                    CandidateIntegrityConflictActionV1::ReconciliationRequired
                );
                let snapshot = registry.snapshot(candidate).expect("race snapshot");
                assert_eq!(
                    snapshot.lifecycle_phase,
                    CandidateLifecyclePhaseV1::SubmitStarted
                );
                assert!(snapshot.reconciliation_required);
                submit_guard
                    .mark_confirmed()
                    .expect("real confirmation remains authoritative after submit won");
            }
            Err(CandidateIntegrityErrorV1::GenerationChanged { .. })
            | Err(CandidateIntegrityErrorV1::NotReady(_)) => {
                assert_eq!(
                    conflict_result.action,
                    CandidateIntegrityConflictActionV1::CancelExecutionBeforeSubmit
                );
                let snapshot = registry.snapshot(candidate).expect("race snapshot");
                assert_eq!(
                    snapshot.lifecycle_phase,
                    CandidateLifecyclePhaseV1::TerminalBuyNotSubmitted
                );
                assert!(!snapshot.reconciliation_required);
            }
            other => panic!("unexpected conflict-vs-submit race result: {other:?}"),
        }
    }
}

#[test]
fn shadow_bounded_audit_overflow_never_rewrites_confirmed_history() {
    let candidate = candidate();
    let registry = ready_registry_with_limits(
        candidate,
        CandidateIntegrityRegistryLimitsV1 {
            max_candidates: 1,
            max_audit_markers_per_candidate: 2,
        },
    );
    let _submit_guard = confirmed_position(&registry, candidate);

    for (outcome, hash_byte) in [
        (CandidateIntegrityOutcomeV1::SourceReconciliationConflict, 2),
        (CandidateIntegrityOutcomeV1::AccountProviderConflict, 3),
        (CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete, 4),
    ] {
        let result = registry
            .record_signal(signal(candidate, outcome, hash_byte))
            .expect("bounded audit overflow must not reject lifecycle handling");
        assert_eq!(
            result.action,
            CandidateIntegrityConflictActionV1::ConfirmedPositionQuarantined
        );
        assert_eq!(
            result.snapshot.lifecycle_phase,
            CandidateLifecyclePhaseV1::ConfirmedOpenPosition
        );
    }

    let snapshot = registry.snapshot(candidate).expect("overflow snapshot");
    assert_eq!(snapshot.audit_markers.len(), 2);
    assert!(!snapshot.audit_evidence_complete);
    assert_eq!(
        snapshot.lifecycle_phase,
        CandidateLifecyclePhaseV1::ConfirmedOpenPosition
    );
    assert!(snapshot.witness_quarantined);
    assert!(!snapshot.reconciliation_required);
}
