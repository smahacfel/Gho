//! Independent adversarial validation of the public PR1D Pump Observation
//! Ledger API.
//!
//! These tests deliberately use only public `ghost_core` exports. They do not
//! redefine the ledger contract or share private helpers with its unit tests.

use ghost_core::{
    CandidateIntegrityOutcomeV1, CanonicalPumpOrderKeyV1, ObservationProvenanceV1,
    ObservationSourceFamilyV1, ObservedPumpMutationV1, ParsedWitnessCorrelationOutcomeV1,
    ProgramFeeCharge, PumpInstructionLimitV1, PumpMutationClaimsV1, PumpMutationConflictFieldV1,
    PumpMutationFamilyV1, PumpObservationClassificationV1, PumpObservationEvidenceLaneV1,
    PumpObservationLedgerConfigV1, PumpObservationLedgerV1, PumpProviderAgreementV1,
    PumpRouteVariant, PumpTradeSideV1, RawProviderRoleV1, RawPumpMutationLocatorV1,
};
use solana_sdk::{pubkey::Pubkey, signature::Signature};

fn key(byte: u8) -> Pubkey {
    Pubkey::new_from_array([byte; 32])
}

fn signature(byte: u8) -> Signature {
    Signature::from([byte; 64])
}

fn locator(signature: Signature, ordinal: u32) -> RawPumpMutationLocatorV1 {
    RawPumpMutationLocatorV1 {
        program_id: key(250),
        signature,
        outer_instruction_index: 2,
        inner_instruction_path: vec![0, ordinal as u16],
        semantic_event_ordinal: ordinal,
    }
}

fn canonical_order(
    locator: &RawPumpMutationLocatorV1,
    slot: u64,
    tx_index: u32,
) -> CanonicalPumpOrderKeyV1 {
    CanonicalPumpOrderKeyV1 {
        slot,
        tx_index,
        outer_instruction_index: locator.outer_instruction_index,
        inner_instruction_path: locator.inner_instruction_path.clone(),
        semantic_event_ordinal: locator.semantic_event_ordinal,
    }
}

fn complete_claims(curve: Pubkey, mint: Pubkey) -> PumpMutationClaimsV1 {
    PumpMutationClaimsV1 {
        curve: Some(curve),
        mint: Some(mint),
        route_variant: Some(PumpRouteVariant::BuyV2),
        side: Some(PumpTradeSideV1::Buy),
        success: Some(true),
        error_code: Some("ok".to_owned()),
        token_amount_units: Some(42),
        instruction_limit: Some(PumpInstructionLimitV1::MaxWalletDebitLamports(100)),
        reported_curve_quote_lamports: Some(90),
        reported_wallet_delta_lamports: Some(100),
        reported_fee_breakdown: Some(vec![ProgramFeeCharge {
            component_id: "protocol".to_owned(),
            amount: 10,
        }]),
        reported_post_state_hash_blake3: Some([7; 32]),
    }
}

fn provenance(
    source_family: ObservationSourceFamilyV1,
    provider_id: &str,
    hash_byte: u8,
    received_at_monotonic_ns: u64,
) -> ObservationProvenanceV1 {
    ObservationProvenanceV1 {
        source_family,
        source_id: match source_family {
            ObservationSourceFamilyV1::RawYellowstone => "yellowstone-global",
            ObservationSourceFamilyV1::ParsedNln => "nln-pumpfun",
        }
        .to_owned(),
        provider_id: provider_id.to_owned(),
        schema_id: match source_family {
            ObservationSourceFamilyV1::RawYellowstone => {
                "yellowstone.subscribe_update_transaction.prost.v1"
            }
            ObservationSourceFamilyV1::ParsedNln => "nln.pumpfun.trade.json.v1",
        }
        .to_owned(),
        payload_hash_blake3: [hash_byte; 32],
        received_at_monotonic_ns,
    }
}

fn primary_raw(
    locator: RawPumpMutationLocatorV1,
    claims: PumpMutationClaimsV1,
    hash_byte: u8,
    mutation_count: Option<u32>,
) -> ObservedPumpMutationV1 {
    ObservedPumpMutationV1 {
        mutation_family: PumpMutationFamilyV1::Trade,
        signature: locator.signature,
        canonical_order: Some(canonical_order(&locator, 1_000, 0)),
        locator_hint: Some(locator),
        raw_transaction_mutation_count: mutation_count,
        claims,
        raw_provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
        provenance: provenance(
            ObservationSourceFamilyV1::RawYellowstone,
            "raw-primary",
            hash_byte,
            10,
        ),
    }
}

fn secondary_raw(
    locator: RawPumpMutationLocatorV1,
    claims: PumpMutationClaimsV1,
    provider_id: &str,
    hash_byte: u8,
) -> ObservedPumpMutationV1 {
    ObservedPumpMutationV1 {
        mutation_family: PumpMutationFamilyV1::Trade,
        signature: locator.signature,
        canonical_order: Some(canonical_order(&locator, 1_000, 0)),
        locator_hint: Some(locator),
        raw_transaction_mutation_count: Some(1),
        claims,
        raw_provider_role: Some(RawProviderRoleV1::SecondaryWitness),
        provenance: provenance(
            ObservationSourceFamilyV1::RawYellowstone,
            provider_id,
            hash_byte,
            11,
        ),
    }
}

fn parsed_nln(
    signature: Signature,
    locator_hint: Option<RawPumpMutationLocatorV1>,
    claims: PumpMutationClaimsV1,
    provider_id: &str,
    hash_byte: u8,
) -> ObservedPumpMutationV1 {
    ObservedPumpMutationV1 {
        mutation_family: PumpMutationFamilyV1::Trade,
        signature,
        locator_hint,
        canonical_order: None,
        raw_transaction_mutation_count: None,
        claims,
        raw_provider_role: None,
        provenance: provenance(
            ObservationSourceFamilyV1::ParsedNln,
            provider_id,
            hash_byte,
            12,
        ),
    }
}

fn small_config(
    primary: usize,
    pending: usize,
    correlated: usize,
    conflicts: usize,
) -> PumpObservationLedgerConfigV1 {
    PumpObservationLedgerConfigV1 {
        correlation_window_ns: 10,
        max_primary_canonical_mutations: primary,
        max_pending_witnesses: pending,
        max_correlated_witnesses_per_mutation: correlated,
        max_retained_conflicts: conflicts,
        max_terminal_canonical_tombstones: 8,
    }
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    fn visit<T: Clone>(
        items: &[T],
        used: &mut [bool],
        current: &mut Vec<T>,
        output: &mut Vec<Vec<T>>,
    ) {
        if current.len() == items.len() {
            output.push(current.clone());
            return;
        }

        for index in 0..items.len() {
            if used[index] {
                continue;
            }
            used[index] = true;
            current.push(items[index].clone());
            visit(items, used, current, output);
            current.pop();
            used[index] = false;
        }
    }

    let mut output = Vec::new();
    visit(
        items,
        &mut vec![false; items.len()],
        &mut Vec::with_capacity(items.len()),
        &mut output,
    );
    output
}

fn deterministic_shuffle<T: Clone>(items: &[T], mut state: u64) -> Vec<T> {
    let mut shuffled = items.to_vec();
    for upper in (1..shuffled.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let index = (state as usize) % (upper + 1);
        shuffled.swap(upper, index);
    }
    shuffled
}

#[test]
fn exact_raw_nln_arrival_orders_converge_and_raw_applies_immediately() {
    let sig = signature(1);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(1), key(2));
    let raw = primary_raw(loc.clone(), claims.clone(), 1, Some(1));
    let nln = parsed_nln(sig, Some(loc), claims, "nln-a", 2);

    let mut raw_first = PumpObservationLedgerV1::default();
    assert!(raw_first
        .observe(raw.clone(), 1)
        .observation_decision
        .did_canonical_apply());
    let raw_first_witness = raw_first.observe(nln.clone(), 2).observation_decision;
    assert_eq!(
        raw_first_witness.correlation,
        Some(ParsedWitnessCorrelationOutcomeV1::ExactStructuralMatch)
    );

    let mut nln_first = PumpObservationLedgerV1::default();
    assert!(!nln_first
        .observe(nln, 1)
        .observation_decision
        .did_canonical_apply());
    let raw_result = nln_first.observe(raw, 2);
    assert!(raw_result.observation_decision.did_canonical_apply());
    assert_eq!(raw_result.derived_decisions.len(), 1);
    assert_eq!(
        raw_result.derived_decisions[0].correlation,
        Some(ParsedWitnessCorrelationOutcomeV1::ExactStructuralMatch)
    );

    assert_eq!(raw_first.snapshot(), nln_first.snapshot());
    assert_eq!(
        raw_first.retained_conflicts(),
        nln_first.retained_conflicts()
    );
}

#[test]
fn conflicting_exact_raw_nln_arrival_orders_converge_without_second_apply() {
    let sig = signature(2);
    let loc = locator(sig, 0);
    let raw_claims = complete_claims(key(3), key(4));
    let mut nln_claims = raw_claims.clone();
    nln_claims.token_amount_units = Some(43);
    let raw = primary_raw(loc.clone(), raw_claims, 3, Some(1));
    let nln = parsed_nln(sig, Some(loc), nln_claims, "nln-conflict", 4);

    let mut raw_first = PumpObservationLedgerV1::default();
    assert!(raw_first
        .observe(raw.clone(), 1)
        .observation_decision
        .did_canonical_apply());
    let first_conflict = raw_first.observe(nln.clone(), 2).observation_decision;
    assert_eq!(
        first_conflict.classification,
        PumpObservationClassificationV1::SourceReconciliationConflict
    );

    let mut nln_first = PumpObservationLedgerV1::default();
    assert!(!nln_first
        .observe(nln, 1)
        .observation_decision
        .did_canonical_apply());
    let raw_result = nln_first.observe(raw, 2);
    assert!(raw_result.observation_decision.did_canonical_apply());
    assert_eq!(raw_result.derived_decisions.len(), 1);
    assert_eq!(
        raw_result.derived_decisions[0].classification,
        PumpObservationClassificationV1::SourceReconciliationConflict
    );

    assert_eq!(raw_first.snapshot(), nln_first.snapshot());
    assert_eq!(
        raw_first.retained_conflicts(),
        nln_first.retained_conflicts()
    );
}

#[test]
fn singleton_arrival_orders_finalize_to_the_same_witness_outcome() {
    let sig = signature(3);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(5), key(6));
    let raw = primary_raw(loc, claims.clone(), 5, Some(1));
    let nln = parsed_nln(sig, None, claims, "nln-singleton", 6);

    let mut raw_first =
        PumpObservationLedgerV1::try_new(small_config(8, 8, 8, 8)).expect("valid config");
    assert!(raw_first
        .observe(raw.clone(), 1)
        .observation_decision
        .did_canonical_apply());
    assert!(!raw_first
        .observe(nln.clone(), 2)
        .observation_decision
        .did_canonical_apply());
    let raw_first_final = raw_first.finalize_expired(12);

    let mut nln_first =
        PumpObservationLedgerV1::try_new(small_config(8, 8, 8, 8)).expect("valid config");
    assert!(!nln_first
        .observe(nln, 1)
        .observation_decision
        .did_canonical_apply());
    assert!(nln_first
        .observe(raw, 2)
        .observation_decision
        .did_canonical_apply());
    let nln_first_final = nln_first.finalize_expired(12);

    assert_eq!(raw_first_final, nln_first_final);
    assert_eq!(raw_first_final.len(), 1);
    assert_eq!(
        raw_first_final[0].correlation,
        Some(ParsedWitnessCorrelationOutcomeV1::UniqueSignatureSingletonMatch)
    );
    assert_eq!(raw_first.snapshot(), nln_first.snapshot());
}

#[test]
fn late_raw_singleton_correlates_retained_locatorless_witness_without_second_apply() {
    let sig = signature(180);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(71), key(72));
    let witness = parsed_nln(sig, None, claims.clone(), "nln-late-singleton", 181);
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(4, 4, 4, 4)).expect("valid config");

    ledger.observe(witness.clone(), 1);
    let historical = ledger.finalize_expired(11);
    assert_eq!(historical.len(), 1);
    assert_eq!(
        historical[0].classification,
        PumpObservationClassificationV1::UnmatchableParsedWitness
    );
    assert_eq!(
        historical[0].correlation,
        Some(ParsedWitnessCorrelationOutcomeV1::Unmatchable)
    );
    assert_eq!(ledger.snapshot().finalized_unassigned_witness_count, 1);

    let raw = ledger.observe(primary_raw(loc, claims, 182, Some(1)), 12);
    assert!(raw.observation_decision.did_canonical_apply());
    assert_eq!(raw.derived_decisions.len(), 1);
    assert_eq!(
        raw.derived_decisions[0].classification,
        PumpObservationClassificationV1::UniqueSignatureSingletonMatch
    );
    assert_eq!(
        raw.derived_decisions[0].correlation,
        Some(ParsedWitnessCorrelationOutcomeV1::UniqueSignatureSingletonMatch)
    );
    assert_eq!(
        raw.derived_decisions[0].provider_agreement,
        PumpProviderAgreementV1::PrimarySecondaryAgreement
    );
    assert!(!raw.derived_decisions[0].did_canonical_apply());
    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.canonical_mutation_count, 1);
    assert_eq!(snapshot.pending_witness_count, 0);
    assert_eq!(snapshot.finalized_unassigned_witness_count, 0);

    let replay_pending = ledger.observe(witness, 13).observation_decision;
    assert_eq!(
        replay_pending.classification,
        PumpObservationClassificationV1::ParsedWitnessPending
    );
    let replay = ledger.finalize_expired(23);
    assert_eq!(replay.len(), 1);
    assert_eq!(
        replay[0].classification,
        PumpObservationClassificationV1::ExactDuplicate
    );
    assert_eq!(
        replay[0].correlation,
        Some(ParsedWitnessCorrelationOutcomeV1::UniqueSignatureSingletonMatch)
    );
    assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
}

#[test]
fn public_api_classifies_exact_singleton_ambiguous_and_unmatchable() {
    let exact_sig = signature(4);
    let exact_loc = locator(exact_sig, 0);
    let exact_claims = complete_claims(key(7), key(8));
    let mut exact = PumpObservationLedgerV1::default();
    exact.observe(
        primary_raw(exact_loc.clone(), exact_claims.clone(), 7, Some(1)),
        1,
    );
    let exact_decision = exact
        .observe(
            parsed_nln(exact_sig, Some(exact_loc), exact_claims, "nln-exact", 8),
            2,
        )
        .observation_decision;
    assert_eq!(
        exact_decision.correlation,
        Some(ParsedWitnessCorrelationOutcomeV1::ExactStructuralMatch)
    );

    let singleton_sig = signature(5);
    let singleton_claims = complete_claims(key(9), key(10));
    let mut singleton =
        PumpObservationLedgerV1::try_new(small_config(8, 8, 8, 8)).expect("valid config");
    singleton.observe(
        primary_raw(
            locator(singleton_sig, 0),
            singleton_claims.clone(),
            9,
            Some(1),
        ),
        1,
    );
    singleton.observe(
        parsed_nln(singleton_sig, None, singleton_claims, "nln-singleton", 10),
        2,
    );
    assert_eq!(
        singleton.finalize_expired(12)[0].correlation,
        Some(ParsedWitnessCorrelationOutcomeV1::UniqueSignatureSingletonMatch)
    );

    let ambiguous_sig = signature(6);
    let ambiguous_claims = complete_claims(key(11), key(12));
    let mut ambiguous =
        PumpObservationLedgerV1::try_new(small_config(8, 8, 8, 8)).expect("valid config");
    ambiguous.observe(
        primary_raw(
            locator(ambiguous_sig, 0),
            ambiguous_claims.clone(),
            11,
            Some(2),
        ),
        1,
    );
    ambiguous.observe(
        primary_raw(
            locator(ambiguous_sig, 1),
            ambiguous_claims.clone(),
            12,
            Some(2),
        ),
        2,
    );
    ambiguous.observe(
        parsed_nln(ambiguous_sig, None, ambiguous_claims, "nln-ambiguous", 13),
        3,
    );
    assert_eq!(
        ambiguous.finalize_expired(13)[0].correlation,
        Some(ParsedWitnessCorrelationOutcomeV1::Ambiguous)
    );

    let unmatchable_sig = signature(7);
    let mut unmatchable =
        PumpObservationLedgerV1::try_new(small_config(8, 8, 8, 8)).expect("valid config");
    unmatchable.observe(
        parsed_nln(
            unmatchable_sig,
            None,
            complete_claims(key(13), key(14)),
            "nln-only",
            14,
        ),
        1,
    );
    let unmatchable_decision = unmatchable.finalize_expired(11);
    assert_eq!(
        unmatchable_decision[0].correlation,
        Some(ParsedWitnessCorrelationOutcomeV1::Unmatchable)
    );
    assert_eq!(unmatchable.snapshot().canonical_mutation_count, 0);
}

#[test]
fn finalized_ambiguous_and_unmatchable_witnesses_remain_exact_replay_evidence() {
    let ambiguous_sig = signature(153);
    let claims = complete_claims(key(57), key(58));
    let ambiguous_witness = parsed_nln(
        ambiguous_sig,
        None,
        claims.clone(),
        "nln-ambiguous-replay",
        154,
    );
    let mut ambiguous =
        PumpObservationLedgerV1::try_new(small_config(4, 4, 4, 4)).expect("valid config");
    ambiguous.observe(
        primary_raw(locator(ambiguous_sig, 0), claims.clone(), 155, Some(2)),
        0,
    );
    ambiguous.observe(
        primary_raw(locator(ambiguous_sig, 1), claims.clone(), 156, Some(2)),
        0,
    );
    ambiguous.observe(ambiguous_witness.clone(), 1);
    assert_eq!(
        ambiguous.finalize_expired(11)[0].classification,
        PumpObservationClassificationV1::AmbiguousParsedWitness
    );
    assert_eq!(
        ambiguous
            .observe(ambiguous_witness, 12)
            .observation_decision
            .classification,
        PumpObservationClassificationV1::ExactDuplicate,
        "an ambiguous witness remains bounded ledger evidence after finalization"
    );

    let unmatchable_sig = signature(157);
    let unmatchable_witness =
        parsed_nln(unmatchable_sig, None, claims, "nln-unmatchable-replay", 158);
    let mut unmatchable =
        PumpObservationLedgerV1::try_new(small_config(4, 4, 4, 4)).expect("valid config");
    unmatchable.observe(unmatchable_witness.clone(), 1);
    assert_eq!(
        unmatchable.finalize_expired(11)[0].classification,
        PumpObservationClassificationV1::UnmatchableParsedWitness
    );
    assert_eq!(
        unmatchable
            .observe(unmatchable_witness, 12)
            .observation_decision
            .classification,
        PumpObservationClassificationV1::ExactDuplicate,
        "an unmatchable witness remains bounded ledger evidence after finalization"
    );
}

#[test]
fn two_mutations_with_one_signature_both_apply_and_duplicate_does_not_reapply() {
    let sig = signature(8);
    let claims = complete_claims(key(15), key(16));
    let first = primary_raw(locator(sig, 0), claims.clone(), 15, Some(2));
    let second = primary_raw(locator(sig, 1), claims, 16, Some(2));
    let mut ledger = PumpObservationLedgerV1::default();

    assert!(ledger
        .observe(first.clone(), 1)
        .observation_decision
        .did_canonical_apply());
    assert!(ledger
        .observe(second, 2)
        .observation_decision
        .did_canonical_apply());
    let duplicate = ledger.observe(first, 999).observation_decision;
    assert_eq!(
        duplicate.classification,
        PumpObservationClassificationV1::ExactDuplicate
    );
    assert!(!duplicate.did_canonical_apply());
    assert_eq!(
        duplicate.provider_agreement,
        PumpProviderAgreementV1::NotObserved,
        "a replay from the primary provider is not primary-secondary agreement"
    );
    assert_eq!(ledger.snapshot().canonical_mutation_count, 2);
    assert_eq!(ledger.snapshot().exact_duplicate_count, 1);
}

#[test]
fn exact_replay_of_a_conflicting_witness_does_not_turn_conflict_into_agreement() {
    let sig = signature(134);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(49), key(50));
    let mut conflicting_claims = claims.clone();
    conflicting_claims.token_amount_units = Some(43);
    let conflicting_witness = parsed_nln(
        sig,
        Some(loc.clone()),
        conflicting_claims,
        "nln-replay",
        135,
    );
    let mut ledger = PumpObservationLedgerV1::default();

    assert!(ledger
        .observe(primary_raw(loc, claims, 136, Some(1)), 1)
        .observation_decision
        .did_canonical_apply());
    let conflict = ledger
        .observe(conflicting_witness.clone(), 2)
        .observation_decision;
    assert_eq!(
        conflict.provider_agreement,
        PumpProviderAgreementV1::PrimarySecondaryConflict
    );

    let replay = ledger.observe(conflicting_witness, 3).observation_decision;
    assert_eq!(
        replay.classification,
        PumpObservationClassificationV1::ExactDuplicate
    );
    assert_eq!(
        replay.provider_agreement,
        PumpProviderAgreementV1::PrimarySecondaryConflict,
        "an exact replay must preserve the already-established source relationship"
    );
    assert_eq!(
        replay.conflict_fields,
        vec![PumpMutationConflictFieldV1::TokenAmountUnits]
    );
    assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
    assert_eq!(ledger.snapshot().conflict_count, 1);
    assert_eq!(ledger.snapshot().retained_conflict_count, 1);
}

#[test]
fn receive_timestamp_does_not_change_exact_observation_identity() {
    let sig = signature(165);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(63), key(64));
    let original = primary_raw(loc, claims, 166, Some(1));
    let mut replay = original.clone();
    replay.provenance.received_at_monotonic_ns = 999_999;
    let mut ledger = PumpObservationLedgerV1::default();

    assert!(ledger
        .observe(original, 1)
        .observation_decision
        .did_canonical_apply());
    let replay_decision = ledger.observe(replay, 999_999).observation_decision;
    assert_eq!(
        replay_decision.classification,
        PumpObservationClassificationV1::ExactDuplicate
    );
    assert_eq!(
        replay_decision.provider_agreement,
        PumpProviderAgreementV1::NotObserved
    );
    assert!(!replay_decision.did_canonical_apply());
    assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
    assert_eq!(ledger.snapshot().exact_duplicate_count, 1);
}

#[test]
fn same_primary_identity_order_contradiction_is_retained_once_then_deduplicated() {
    let sig = signature(167);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(65), key(66));
    let original = primary_raw(loc, claims, 168, Some(1));
    let mut contradictory = original.clone();
    contradictory
        .canonical_order
        .as_mut()
        .expect("primary canonical order")
        .tx_index = 9;
    let mut ledger = PumpObservationLedgerV1::default();

    assert!(ledger
        .observe(original, 1)
        .observation_decision
        .did_canonical_apply());
    let first_conflict = ledger
        .observe(contradictory.clone(), 2)
        .observation_decision;
    assert_eq!(
        first_conflict.classification,
        PumpObservationClassificationV1::SourceReconciliationConflict
    );
    assert_eq!(
        first_conflict.provider_agreement,
        PumpProviderAgreementV1::NotObserved,
        "a contradiction from the sole primary is not primary-secondary disagreement"
    );
    assert_eq!(
        first_conflict.conflict_fields,
        vec![PumpMutationConflictFieldV1::CanonicalOrder]
    );
    assert_eq!(
        first_conflict
            .candidate_integrity_signal
            .as_ref()
            .map(|signal| signal.outcome),
        Some(CandidateIntegrityOutcomeV1::SourceReconciliationConflict)
    );
    assert!(!first_conflict.did_canonical_apply());
    assert_eq!(ledger.snapshot().conflict_count, 1);
    assert_eq!(ledger.snapshot().retained_conflict_count, 1);

    let replay = ledger.observe(contradictory, 3).observation_decision;
    assert_eq!(
        replay.classification,
        PumpObservationClassificationV1::ExactDuplicate
    );
    assert_eq!(
        replay.provider_agreement,
        PumpProviderAgreementV1::NotObserved
    );
    assert_eq!(
        replay.conflict_fields,
        vec![PumpMutationConflictFieldV1::CanonicalOrder]
    );
    assert!(!replay.did_canonical_apply());
    assert_eq!(
        ledger.snapshot().conflict_count,
        1,
        "replaying the contradictory normalization must not count a second conflict"
    );
    assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
}

#[test]
fn new_primary_payload_for_existing_locator_never_impersonates_secondary() {
    let claims = complete_claims(key(67), key(68));

    let agreement_sig = signature(169);
    let agreement_loc = locator(agreement_sig, 0);
    let original = primary_raw(agreement_loc, claims.clone(), 170, Some(1));
    let mut semantically_equal_new_payload = original.clone();
    semantically_equal_new_payload
        .provenance
        .payload_hash_blake3 = [171; 32];
    let mut agreement_ledger = PumpObservationLedgerV1::default();
    agreement_ledger.observe(original, 1);
    let agreement = agreement_ledger
        .observe(semantically_equal_new_payload, 2)
        .observation_decision;
    assert_eq!(
        agreement.classification,
        PumpObservationClassificationV1::SameMutationAgreement
    );
    assert_eq!(
        agreement.provider_agreement,
        PumpProviderAgreementV1::NotObserved
    );
    assert!(agreement.conflict_fields.is_empty());
    assert!(!agreement.did_canonical_apply());
    assert_eq!(agreement_ledger.snapshot().canonical_mutation_count, 1);
    assert_eq!(agreement_ledger.snapshot().conflict_count, 0);

    let conflict_sig = signature(172);
    let conflict_loc = locator(conflict_sig, 0);
    let original = primary_raw(conflict_loc, claims.clone(), 173, Some(1));
    let mut conflicting_new_payload = original.clone();
    conflicting_new_payload.provenance.payload_hash_blake3 = [174; 32];
    conflicting_new_payload.claims.token_amount_units = Some(43);
    let mut conflict_ledger = PumpObservationLedgerV1::default();
    conflict_ledger.observe(original, 1);
    let conflict = conflict_ledger
        .observe(conflicting_new_payload, 2)
        .observation_decision;
    assert_eq!(
        conflict.classification,
        PumpObservationClassificationV1::SourceReconciliationConflict
    );
    assert_eq!(
        conflict.provider_agreement,
        PumpProviderAgreementV1::NotObserved
    );
    assert_eq!(
        conflict.conflict_fields,
        vec![PumpMutationConflictFieldV1::TokenAmountUnits]
    );
    assert_eq!(
        conflict
            .candidate_integrity_signal
            .as_ref()
            .map(|signal| signal.outcome),
        Some(CandidateIntegrityOutcomeV1::SourceReconciliationConflict)
    );
    assert!(!conflict.did_canonical_apply());
    assert_eq!(conflict_ledger.snapshot().canonical_mutation_count, 1);
    assert_eq!(conflict_ledger.snapshot().conflict_count, 1);
}

#[derive(Clone, Copy, Debug)]
enum ClaimCase {
    Curve,
    Mint,
    RouteVariant,
    Side,
    Success,
    ErrorCode,
    TokenAmountUnits,
    InstructionLimit,
    ReportedCurveQuoteLamports,
    ReportedWalletDeltaLamports,
    ReportedFeeBreakdown,
    ReportedPostStateHashBlake3,
}

const CLAIM_CASES: &[(ClaimCase, PumpMutationConflictFieldV1)] = &[
    (ClaimCase::Curve, PumpMutationConflictFieldV1::Curve),
    (ClaimCase::Mint, PumpMutationConflictFieldV1::Mint),
    (
        ClaimCase::RouteVariant,
        PumpMutationConflictFieldV1::RouteVariant,
    ),
    (ClaimCase::Side, PumpMutationConflictFieldV1::Side),
    (ClaimCase::Success, PumpMutationConflictFieldV1::Success),
    (ClaimCase::ErrorCode, PumpMutationConflictFieldV1::ErrorCode),
    (
        ClaimCase::TokenAmountUnits,
        PumpMutationConflictFieldV1::TokenAmountUnits,
    ),
    (
        ClaimCase::InstructionLimit,
        PumpMutationConflictFieldV1::InstructionLimit,
    ),
    (
        ClaimCase::ReportedCurveQuoteLamports,
        PumpMutationConflictFieldV1::ReportedCurveQuoteLamports,
    ),
    (
        ClaimCase::ReportedWalletDeltaLamports,
        PumpMutationConflictFieldV1::ReportedWalletDeltaLamports,
    ),
    (
        ClaimCase::ReportedFeeBreakdown,
        PumpMutationConflictFieldV1::ReportedFeeBreakdown,
    ),
    (
        ClaimCase::ReportedPostStateHashBlake3,
        PumpMutationConflictFieldV1::ReportedPostStateHashBlake3,
    ),
];

fn make_concrete_difference(case: ClaimCase, claims: &mut PumpMutationClaimsV1) {
    match case {
        ClaimCase::Curve => claims.curve = Some(key(201)),
        ClaimCase::Mint => claims.mint = Some(key(202)),
        ClaimCase::RouteVariant => claims.route_variant = Some(PumpRouteVariant::SellV2),
        ClaimCase::Side => claims.side = Some(PumpTradeSideV1::Sell),
        ClaimCase::Success => claims.success = Some(false),
        ClaimCase::ErrorCode => claims.error_code = Some("different".to_owned()),
        ClaimCase::TokenAmountUnits => claims.token_amount_units = Some(43),
        ClaimCase::InstructionLimit => {
            claims.instruction_limit = Some(PumpInstructionLimitV1::MinWalletCreditLamports(101));
        }
        ClaimCase::ReportedCurveQuoteLamports => {
            claims.reported_curve_quote_lamports = Some(91);
        }
        ClaimCase::ReportedWalletDeltaLamports => {
            claims.reported_wallet_delta_lamports = Some(101);
        }
        ClaimCase::ReportedFeeBreakdown => {
            claims.reported_fee_breakdown = Some(vec![ProgramFeeCharge {
                component_id: "protocol".to_owned(),
                amount: 11,
            }]);
        }
        ClaimCase::ReportedPostStateHashBlake3 => {
            claims.reported_post_state_hash_blake3 = Some([8; 32]);
        }
    }
}

fn clear_claim(case: ClaimCase, claims: &mut PumpMutationClaimsV1) {
    match case {
        ClaimCase::Curve => claims.curve = None,
        ClaimCase::Mint => claims.mint = None,
        ClaimCase::RouteVariant => claims.route_variant = None,
        ClaimCase::Side => claims.side = None,
        ClaimCase::Success => claims.success = None,
        ClaimCase::ErrorCode => claims.error_code = None,
        ClaimCase::TokenAmountUnits => claims.token_amount_units = None,
        ClaimCase::InstructionLimit => claims.instruction_limit = None,
        ClaimCase::ReportedCurveQuoteLamports => {
            claims.reported_curve_quote_lamports = None;
        }
        ClaimCase::ReportedWalletDeltaLamports => {
            claims.reported_wallet_delta_lamports = None;
        }
        ClaimCase::ReportedFeeBreakdown => claims.reported_fee_breakdown = None,
        ClaimCase::ReportedPostStateHashBlake3 => {
            claims.reported_post_state_hash_blake3 = None;
        }
    }
}

#[test]
fn every_material_claim_concrete_difference_is_a_typed_conflict() {
    for (index, (case, expected_field)) in CLAIM_CASES.iter().enumerate() {
        let sig = signature(20 + index as u8);
        let loc = locator(sig, 0);
        let raw_claims = complete_claims(key(17), key(18));
        let mut witness_claims = raw_claims.clone();
        make_concrete_difference(*case, &mut witness_claims);
        let mut ledger = PumpObservationLedgerV1::default();
        assert!(ledger
            .observe(primary_raw(loc.clone(), raw_claims, 40, Some(1)), 1)
            .observation_decision
            .did_canonical_apply());

        let conflict = ledger
            .observe(
                parsed_nln(
                    sig,
                    Some(loc),
                    witness_claims,
                    &format!("nln-conflict-{index}"),
                    41 + index as u8,
                ),
                2,
            )
            .observation_decision;
        assert_eq!(
            conflict.classification,
            PumpObservationClassificationV1::SourceReconciliationConflict,
            "case={case:?}"
        );
        assert_eq!(
            conflict.conflict_fields,
            vec![*expected_field],
            "case={case:?}"
        );
        assert_eq!(
            conflict
                .candidate_integrity_signal
                .as_ref()
                .map(|signal| signal.outcome),
            Some(CandidateIntegrityOutcomeV1::SourceReconciliationConflict),
            "case={case:?}"
        );
        assert!(!conflict.did_canonical_apply());
        assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
    }
}

#[test]
fn unknown_against_concrete_is_agreement_for_every_material_claim() {
    for (index, (case, _)) in CLAIM_CASES.iter().enumerate() {
        let sig = signature(40 + index as u8);
        let loc = locator(sig, 0);
        let raw_claims = complete_claims(key(19), key(20));
        let mut witness_claims = raw_claims.clone();
        clear_claim(*case, &mut witness_claims);
        let mut ledger = PumpObservationLedgerV1::default();
        ledger.observe(primary_raw(loc.clone(), raw_claims, 60, Some(1)), 1);

        let agreement = ledger
            .observe(
                parsed_nln(
                    sig,
                    Some(loc),
                    witness_claims,
                    &format!("nln-unknown-{index}"),
                    61 + index as u8,
                ),
                2,
            )
            .observation_decision;
        assert_eq!(
            agreement.provider_agreement,
            PumpProviderAgreementV1::PrimarySecondaryAgreement,
            "case={case:?}"
        );
        assert!(agreement.conflict_fields.is_empty(), "case={case:?}");
        assert!(agreement.candidate_integrity_signal.is_none());
        assert!(!agreement.did_canonical_apply());
    }
}

#[test]
fn different_cross_source_payload_hashes_with_same_claims_are_agreement() {
    let sig = signature(60);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(21), key(22));
    let mut ledger = PumpObservationLedgerV1::default();
    ledger.observe(primary_raw(loc.clone(), claims.clone(), 90, Some(1)), 1);
    let agreement = ledger
        .observe(
            parsed_nln(sig, Some(loc), claims, "nln-different-format", 91),
            2,
        )
        .observation_decision;

    assert_eq!(
        agreement.provider_agreement,
        PumpProviderAgreementV1::PrimarySecondaryAgreement
    );
    assert!(agreement.conflict_fields.is_empty());
    assert_eq!(ledger.snapshot().conflict_count, 0);
}

#[test]
fn mutation_family_and_canonical_order_are_material_conflicts() {
    let claims = complete_claims(key(51), key(52));

    let family_sig = signature(137);
    let family_locator = locator(family_sig, 0);
    let mut family_witness = parsed_nln(
        family_sig,
        Some(family_locator.clone()),
        claims.clone(),
        "nln-family",
        138,
    );
    family_witness.mutation_family = PumpMutationFamilyV1::InitializePool;
    let mut family_ledger = PumpObservationLedgerV1::default();
    family_ledger.observe(primary_raw(family_locator, claims.clone(), 139, Some(1)), 1);
    let family_conflict = family_ledger
        .observe(family_witness, 2)
        .observation_decision;
    assert_eq!(
        family_conflict.conflict_fields,
        vec![PumpMutationConflictFieldV1::MutationFamily]
    );
    assert_eq!(
        family_conflict.provider_agreement,
        PumpProviderAgreementV1::PrimarySecondaryConflict
    );

    let order_sig = signature(140);
    let order_locator = locator(order_sig, 0);
    let mut order_witness = secondary_raw(
        order_locator.clone(),
        claims.clone(),
        "raw-secondary-order",
        141,
    );
    order_witness
        .canonical_order
        .as_mut()
        .expect("secondary raw order")
        .tx_index = 7;
    let mut order_ledger = PumpObservationLedgerV1::default();
    order_ledger.observe(primary_raw(order_locator, claims, 142, Some(1)), 1);
    let order_conflict = order_ledger.observe(order_witness, 2).observation_decision;
    assert_eq!(
        order_conflict.conflict_fields,
        vec![PumpMutationConflictFieldV1::CanonicalOrder]
    );
    assert_eq!(
        order_conflict.provider_agreement,
        PumpProviderAgreementV1::PrimarySecondaryConflict
    );
}

#[test]
fn exact_observation_permutations_converge_without_extra_canonical_apply() {
    let sig = signature(143);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(53), key(54));
    let primary = primary_raw(loc.clone(), claims.clone(), 144, Some(1));
    let inputs = vec![
        primary.clone(),
        parsed_nln(
            sig,
            Some(loc.clone()),
            claims.clone(),
            "nln-permutation",
            145,
        ),
        secondary_raw(loc, claims, "raw-secondary-permutation", 146),
        primary,
    ];

    let mut expected_snapshot = None;
    let mut expected_conflicts = None;
    for (permutation_index, permutation) in permutations(&inputs).into_iter().enumerate() {
        let mut ledger = PumpObservationLedgerV1::default();
        let mut canonical_applies = 0;
        for (arrival_index, observation) in permutation.into_iter().enumerate() {
            let result = ledger.observe(observation, arrival_index as u64);
            canonical_applies += usize::from(result.observation_decision.did_canonical_apply());
            canonical_applies += result
                .derived_decisions
                .iter()
                .filter(|decision| decision.did_canonical_apply())
                .count();
        }

        assert_eq!(
            canonical_applies, 1,
            "permutation {permutation_index} applied canonical state more than once"
        );
        let snapshot = ledger.snapshot();
        assert_eq!(snapshot.canonical_mutation_count, 1);
        assert_eq!(snapshot.provider_observation_count, 4);
        assert_eq!(snapshot.exact_duplicate_count, 1);
        assert_eq!(snapshot.conflict_count, 0);
        assert_eq!(snapshot.pending_witness_count, 0);
        if let Some(expected) = &expected_snapshot {
            assert_eq!(
                &snapshot, expected,
                "permutation {permutation_index} changed the final snapshot"
            );
        } else {
            expected_snapshot = Some(snapshot);
        }

        let conflicts = ledger.retained_conflicts().to_vec();
        if let Some(expected) = &expected_conflicts {
            assert_eq!(
                &conflicts, expected,
                "permutation {permutation_index} changed retained conflict evidence"
            );
        } else {
            expected_conflicts = Some(conflicts);
        }
    }
}

#[test]
fn conflicting_observation_permutations_converge_without_witness_authority() {
    let sig = signature(147);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(55), key(56));
    let primary = primary_raw(loc.clone(), claims.clone(), 148, Some(1));
    let mut conflicting_claims = claims.clone();
    conflicting_claims.token_amount_units = Some(9_999);
    let inputs = vec![
        primary.clone(),
        parsed_nln(
            sig,
            Some(loc.clone()),
            conflicting_claims,
            "nln-conflict-permutation",
            149,
        ),
        secondary_raw(loc, claims, "raw-secondary-agreement-permutation", 150),
        primary,
    ];

    let mut expected_snapshot = None;
    let mut expected_conflicts = None;
    for (permutation_index, permutation) in permutations(&inputs).into_iter().enumerate() {
        let mut ledger = PumpObservationLedgerV1::default();
        let mut canonical_applies = 0;
        for (arrival_index, observation) in permutation.into_iter().enumerate() {
            let result = ledger.observe(observation, arrival_index as u64);
            canonical_applies += usize::from(result.observation_decision.did_canonical_apply());
            canonical_applies += result
                .derived_decisions
                .iter()
                .filter(|decision| decision.did_canonical_apply())
                .count();
        }

        assert_eq!(
            canonical_applies, 1,
            "permutation {permutation_index} gave a witness canonical authority"
        );
        let snapshot = ledger.snapshot();
        assert_eq!(snapshot.canonical_mutation_count, 1);
        assert_eq!(snapshot.provider_observation_count, 4);
        assert_eq!(snapshot.exact_duplicate_count, 1);
        assert_eq!(snapshot.conflict_count, 1);
        assert_eq!(snapshot.retained_conflict_count, 1);
        assert_eq!(snapshot.pending_witness_count, 0);
        if let Some(expected) = &expected_snapshot {
            assert_eq!(
                &snapshot, expected,
                "permutation {permutation_index} changed the final conflict snapshot"
            );
        } else {
            expected_snapshot = Some(snapshot);
        }

        let conflicts = ledger.retained_conflicts().to_vec();
        if let Some(expected) = &expected_conflicts {
            assert_eq!(
                &conflicts, expected,
                "permutation {permutation_index} changed retained conflict evidence"
            );
        } else {
            expected_conflicts = Some(conflicts);
        }
    }
}

#[test]
fn witness_capacity_pressure_in_every_arrival_order_never_vetoes_primary() {
    let sig = signature(159);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(59), key(60));
    let mut conflicting_claims = claims.clone();
    conflicting_claims.token_amount_units = Some(10_001);
    let inputs = vec![
        primary_raw(loc.clone(), claims.clone(), 160, Some(1)),
        parsed_nln(
            sig,
            Some(loc.clone()),
            conflicting_claims,
            "nln-capacity-permutation",
            161,
        ),
        secondary_raw(loc, claims, "raw-secondary-capacity-permutation", 162),
    ];

    for (permutation_index, permutation) in permutations(&inputs).into_iter().enumerate() {
        let mut ledger =
            PumpObservationLedgerV1::try_new(small_config(1, 1, 1, 1)).expect("valid config");
        let mut canonical_applies = 0;
        for (arrival_index, observation) in permutation.into_iter().enumerate() {
            let result = ledger.observe(observation, arrival_index as u64);
            canonical_applies += usize::from(result.observation_decision.did_canonical_apply());
            canonical_applies += result
                .derived_decisions
                .iter()
                .filter(|decision| decision.did_canonical_apply())
                .count();
        }

        assert_eq!(
            canonical_applies, 1,
            "witness capacity pressure vetoed or duplicated primary in permutation {permutation_index}"
        );
        assert_eq!(
            ledger.snapshot().canonical_mutation_count,
            1,
            "canonical authority drifted in permutation {permutation_index}"
        );
    }
}

#[test]
fn bounded_arrival_fuzz_preserves_exactly_once_and_conflict_cardinality() {
    let sig = signature(175);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(69), key(70));
    let primary = primary_raw(loc.clone(), claims.clone(), 176, Some(1));
    let agreeing_nln = parsed_nln(
        sig,
        Some(loc.clone()),
        claims.clone(),
        "nln-fuzz-agreement",
        177,
    );
    let mut conflicting_claims = claims.clone();
    conflicting_claims.token_amount_units = Some(42_001);
    let conflicting_nln = parsed_nln(
        sig,
        Some(loc.clone()),
        conflicting_claims,
        "nln-fuzz-conflict",
        178,
    );
    let secondary = secondary_raw(loc, claims, "raw-secondary-fuzz", 179);
    let observations = vec![
        primary.clone(),
        primary,
        agreeing_nln.clone(),
        agreeing_nln,
        conflicting_nln.clone(),
        conflicting_nln,
        secondary,
    ];

    let mut expected_snapshot = None;
    for seed in 1..=256_u64 {
        let mut ledger =
            PumpObservationLedgerV1::try_new(small_config(2, 8, 8, 8)).expect("valid config");
        let mut canonical_applies = 0;
        for (arrival_index, observation) in deterministic_shuffle(&observations, seed)
            .into_iter()
            .enumerate()
        {
            let result = ledger.observe(observation, arrival_index as u64);
            canonical_applies += usize::from(result.observation_decision.did_canonical_apply());
            canonical_applies += result
                .derived_decisions
                .iter()
                .filter(|decision| decision.did_canonical_apply())
                .count();
        }

        assert_eq!(
            canonical_applies, 1,
            "canonical apply drift for seed {seed}"
        );
        let snapshot = ledger.snapshot();
        assert_eq!(snapshot.canonical_mutation_count, 1, "seed {seed}");
        assert_eq!(snapshot.provider_observation_count, 7, "seed {seed}");
        assert_eq!(snapshot.exact_duplicate_count, 3, "seed {seed}");
        assert_eq!(snapshot.conflict_count, 1, "seed {seed}");
        assert_eq!(snapshot.retained_conflict_count, 1, "seed {seed}");
        assert_eq!(snapshot.pending_witness_count, 0, "seed {seed}");
        assert_eq!(
            snapshot.finalized_unassigned_witness_count, 0,
            "seed {seed}"
        );
        assert!(snapshot.primary_evidence_complete, "seed {seed}");
        assert!(snapshot.witness_evidence_complete, "seed {seed}");
        assert_eq!(snapshot.evidence_overflow_count, 0, "seed {seed}");
        if let Some(expected) = &expected_snapshot {
            assert_eq!(
                &snapshot, expected,
                "arrival-order fuzz changed the final snapshot for seed {seed}"
            );
        } else {
            expected_snapshot = Some(snapshot);
        }
    }
}

#[test]
fn pending_nln_saturation_records_full_overflow_and_does_not_veto_primary() {
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(2, 1, 2, 2)).expect("valid config");
    let claims = complete_claims(key(23), key(24));
    let first_sig = signature(61);
    let overflow_sig = signature(62);
    ledger.observe(
        parsed_nln(first_sig, None, claims.clone(), "nln-retained", 92),
        1,
    );
    let rejected = parsed_nln(overflow_sig, None, claims.clone(), "nln-overflow", 93);
    let overflow = ledger.observe(rejected.clone(), 2).observation_decision;
    assert_eq!(
        overflow.classification,
        PumpObservationClassificationV1::EvidenceCapacityExceeded
    );
    let later_rejected = parsed_nln(
        signature(151),
        None,
        claims.clone(),
        "nln-later-overflow",
        152,
    );
    assert_eq!(
        ledger
            .observe(later_rejected, 2)
            .observation_decision
            .classification,
        PumpObservationClassificationV1::EvidenceCapacityExceeded
    );
    assert_eq!(
        ledger
            .snapshot()
            .first_evidence_overflow
            .as_ref()
            .expect("first overflow remains visible")
            .first_rejected_observation,
        rejected,
        "later overflows must not overwrite the first rejected observation"
    );

    let primary = ledger.observe(
        primary_raw(locator(overflow_sig, 0), claims, 94, Some(1)),
        3,
    );
    assert!(primary.observation_decision.did_canonical_apply());
    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.canonical_mutation_count, 1);
    assert!(!snapshot.witness_evidence_complete);
    assert_eq!(
        snapshot.evidence_overflow_count, 2,
        "all typed overflow events must remain countable while the first record stays immutable"
    );
    let retained_overflow = snapshot
        .first_evidence_overflow
        .expect("first rejected observation must be retained");
    assert_eq!(
        retained_overflow.lane,
        PumpObservationEvidenceLaneV1::PendingWitness
    );
    assert_eq!(retained_overflow.first_rejected_observation, rejected);
    assert_eq!(retained_overflow.retained_count, 1);
    assert_eq!(retained_overflow.overflow_count, 1);
}

#[test]
fn pending_secondary_saturation_cannot_veto_later_primary() {
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(2, 1, 2, 2)).expect("valid config");
    let claims = complete_claims(key(25), key(26));
    let first_sig = signature(63);
    let overflow_sig = signature(64);
    ledger.observe(
        secondary_raw(locator(first_sig, 0), claims.clone(), "raw-secondary-a", 95),
        1,
    );
    let rejected = secondary_raw(
        locator(overflow_sig, 0),
        claims.clone(),
        "raw-secondary-b",
        96,
    );
    let overflow = ledger.observe(rejected.clone(), 2).observation_decision;
    assert_eq!(
        overflow.classification,
        PumpObservationClassificationV1::EvidenceCapacityExceeded
    );

    let primary = ledger.observe(
        primary_raw(locator(overflow_sig, 0), claims, 97, Some(1)),
        3,
    );
    assert!(primary.observation_decision.did_canonical_apply());
    assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
    assert_eq!(
        ledger
            .snapshot()
            .first_evidence_overflow
            .expect("overflow evidence")
            .first_rejected_observation,
        rejected
    );
}

#[test]
fn correlated_witness_saturation_is_auditable_and_does_not_veto_new_primary() {
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(2, 2, 1, 2)).expect("valid config");
    let sig = signature(65);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(27), key(28));
    ledger.observe(primary_raw(loc.clone(), claims.clone(), 98, Some(1)), 1);
    ledger.observe(
        parsed_nln(sig, Some(loc.clone()), claims.clone(), "nln-retained", 99),
        2,
    );
    let rejected = parsed_nln(
        sig,
        Some(loc),
        claims.clone(),
        "nln-correlated-overflow",
        100,
    );
    let overflow_agreement = ledger.observe(rejected.clone(), 3).observation_decision;
    assert_eq!(
        overflow_agreement.provider_agreement,
        PumpProviderAgreementV1::PrimarySecondaryAgreement
    );
    assert!(!overflow_agreement.evidence_complete);

    let later_sig = signature(66);
    let later_primary = ledger.observe(primary_raw(locator(later_sig, 0), claims, 101, Some(1)), 4);
    assert!(later_primary.observation_decision.did_canonical_apply());
    assert_eq!(ledger.snapshot().canonical_mutation_count, 2);
    let evidence = ledger
        .snapshot()
        .first_evidence_overflow
        .expect("correlated overflow evidence");
    assert_eq!(
        evidence.lane,
        PumpObservationEvidenceLaneV1::CorrelatedWitness
    );
    assert_eq!(evidence.first_rejected_observation, rejected);
}

#[test]
fn correlated_secondary_raw_saturation_does_not_veto_new_primary() {
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(2, 2, 1, 2)).expect("valid config");
    let sig = signature(67);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(41), key(42));
    ledger.observe(primary_raw(loc.clone(), claims.clone(), 118, Some(1)), 1);
    ledger.observe(
        secondary_raw(loc.clone(), claims.clone(), "raw-secondary-retained", 119),
        2,
    );
    let rejected = secondary_raw(
        loc,
        claims.clone(),
        "raw-secondary-correlated-overflow",
        120,
    );
    let overflow_agreement = ledger.observe(rejected.clone(), 3).observation_decision;
    assert_eq!(
        overflow_agreement.classification,
        PumpObservationClassificationV1::SameMutationAgreement
    );
    assert_eq!(overflow_agreement.correlation, None);
    assert!(!overflow_agreement.evidence_complete);

    let later_sig = signature(68);
    assert!(ledger
        .observe(primary_raw(locator(later_sig, 0), claims, 121, Some(1)), 4,)
        .observation_decision
        .did_canonical_apply());
    assert_eq!(ledger.snapshot().canonical_mutation_count, 2);
    let overflow = ledger
        .snapshot()
        .first_evidence_overflow
        .expect("secondary correlated overflow");
    assert_eq!(
        overflow.lane,
        PumpObservationEvidenceLaneV1::CorrelatedWitness
    );
    assert_eq!(overflow.first_rejected_observation, rejected);
}

#[test]
fn conflict_saturation_is_auditable_and_does_not_veto_new_primary() {
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(3, 2, 8, 1)).expect("valid config");
    let claims = complete_claims(key(29), key(30));

    for index in 0..2_u8 {
        let sig = signature(70 + index);
        let loc = locator(sig, 0);
        ledger.observe(
            primary_raw(loc.clone(), claims.clone(), 102 + index, Some(1)),
            u64::from(index),
        );
        let mut conflicting = claims.clone();
        conflicting.token_amount_units = Some(1_000 + u64::from(index));
        let conflict = ledger
            .observe(
                parsed_nln(
                    sig,
                    Some(loc),
                    conflicting,
                    &format!("nln-conflict-{index}"),
                    104 + index,
                ),
                10 + u64::from(index),
            )
            .observation_decision;
        assert_eq!(
            conflict.classification,
            PumpObservationClassificationV1::SourceReconciliationConflict
        );
    }

    let later_sig = signature(72);
    assert!(ledger
        .observe(primary_raw(locator(later_sig, 0), claims, 106, Some(1)), 20,)
        .observation_decision
        .did_canonical_apply());
    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.canonical_mutation_count, 3);
    assert_eq!(snapshot.conflict_count, 2);
    assert_eq!(snapshot.retained_conflict_count, 1);
    assert!(!snapshot.witness_evidence_complete);
    let overflow = snapshot
        .first_evidence_overflow
        .expect("conflict overflow evidence");
    assert_eq!(overflow.lane, PumpObservationEvidenceLaneV1::Conflict);
    assert_eq!(overflow.first_rejected_observation.signature, signature(71));
    assert_eq!(
        overflow.first_rejected_observation.provenance.provider_id,
        "nln-conflict-1"
    );
    assert_eq!(
        overflow
            .first_rejected_observation
            .claims
            .token_amount_units,
        Some(1_001)
    );
}

#[test]
fn secondary_raw_conflict_saturation_does_not_veto_new_primary() {
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(3, 2, 8, 1)).expect("valid config");
    let claims = complete_claims(key(43), key(44));

    for index in 0..2_u8 {
        let sig = signature(82 + index);
        let loc = locator(sig, 0);
        ledger.observe(
            primary_raw(loc.clone(), claims.clone(), 122 + index, Some(1)),
            u64::from(index),
        );
        let mut conflicting = claims.clone();
        conflicting.token_amount_units = Some(2_000 + u64::from(index));
        let conflict = ledger
            .observe(
                secondary_raw(
                    loc,
                    conflicting,
                    &format!("raw-secondary-conflict-{index}"),
                    124 + index,
                ),
                10 + u64::from(index),
            )
            .observation_decision;
        assert_eq!(
            conflict.classification,
            PumpObservationClassificationV1::SourceReconciliationConflict
        );
        assert_eq!(conflict.correlation, None);
        assert!(!conflict.did_canonical_apply());
    }

    assert!(ledger
        .observe(
            primary_raw(locator(signature(84), 0), claims, 126, Some(1)),
            20,
        )
        .observation_decision
        .did_canonical_apply());
    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.canonical_mutation_count, 3);
    assert_eq!(snapshot.conflict_count, 2);
    assert_eq!(snapshot.retained_conflict_count, 1);
    assert!(!snapshot.witness_evidence_complete);
    let overflow = snapshot
        .first_evidence_overflow
        .expect("secondary conflict overflow");
    assert_eq!(overflow.lane, PumpObservationEvidenceLaneV1::Conflict);
    assert_eq!(
        overflow.first_rejected_observation.provenance.provider_id,
        "raw-secondary-conflict-1"
    );
}

#[test]
fn primary_capacity_saturation_fails_closed_and_retains_rejected_primary() {
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(1, 2, 2, 2)).expect("valid config");
    let claims = complete_claims(key(31), key(32));
    ledger.observe(
        primary_raw(locator(signature(73), 0), claims.clone(), 107, Some(1)),
        1,
    );
    let rejected = primary_raw(locator(signature(74), 0), claims, 108, Some(1));
    let decision = ledger.observe(rejected.clone(), 2).observation_decision;

    assert_eq!(
        decision.classification,
        PumpObservationClassificationV1::EvidenceCapacityExceeded
    );
    assert!(!decision.did_canonical_apply());
    assert_eq!(
        decision
            .candidate_integrity_signal
            .as_ref()
            .map(|signal| signal.outcome),
        Some(CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete)
    );
    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.canonical_mutation_count, 1);
    assert!(!snapshot.primary_evidence_complete);
    let overflow = snapshot
        .first_evidence_overflow
        .expect("primary overflow evidence");
    assert_eq!(
        overflow.lane,
        PumpObservationEvidenceLaneV1::PrimaryCanonical
    );
    assert_eq!(overflow.first_rejected_observation, rejected);
}

#[test]
fn missing_locator_or_order_and_zero_mutation_inventory_fail_closed() {
    let claims = complete_claims(key(33), key(34));

    let sig_missing_locator = signature(75);
    let mut missing_locator = primary_raw(
        locator(sig_missing_locator, 0),
        claims.clone(),
        109,
        Some(1),
    );
    missing_locator.locator_hint = None;
    let mut ledger = PumpObservationLedgerV1::default();
    let missing_locator_decision = ledger.observe(missing_locator, 1).observation_decision;
    assert_eq!(
        missing_locator_decision.classification,
        PumpObservationClassificationV1::PrimaryRawCoverageIncomplete
    );
    assert!(!missing_locator_decision.did_canonical_apply());

    let sig_missing_order = signature(76);
    let mut missing_order =
        primary_raw(locator(sig_missing_order, 0), claims.clone(), 110, Some(1));
    missing_order.canonical_order = None;
    let mut ledger = PumpObservationLedgerV1::default();
    let missing_order_decision = ledger.observe(missing_order, 1).observation_decision;
    assert_eq!(
        missing_order_decision.classification,
        PumpObservationClassificationV1::PrimaryRawCoverageIncomplete
    );
    assert!(!missing_order_decision.did_canonical_apply());

    let sig_zero_inventory = signature(77);
    let zero_inventory = primary_raw(locator(sig_zero_inventory, 0), claims, 111, Some(0));
    let mut ledger = PumpObservationLedgerV1::default();
    let zero_inventory_decision = ledger.observe(zero_inventory, 1).observation_decision;
    assert_eq!(
        zero_inventory_decision.classification,
        PumpObservationClassificationV1::PrimaryRawCoverageIncomplete
    );
    assert!(!zero_inventory_decision.did_canonical_apply());
    assert_eq!(ledger.snapshot().canonical_mutation_count, 0);
}

#[test]
fn concrete_transaction_inventory_is_consistent_and_bounds_locator_count() {
    let claims = complete_claims(key(73), key(74));

    let mismatch_sig = signature(183);
    let first = primary_raw(locator(mismatch_sig, 0), claims.clone(), 184, Some(1));
    let mut mismatched_replay = first.clone();
    mismatched_replay.raw_transaction_mutation_count = Some(2);
    let mut mismatch_ledger = PumpObservationLedgerV1::default();
    assert!(mismatch_ledger
        .observe(first, 1)
        .observation_decision
        .did_canonical_apply());
    let mismatch = mismatch_ledger
        .observe(mismatched_replay, 2)
        .observation_decision;
    assert_eq!(
        mismatch.classification,
        PumpObservationClassificationV1::PrimaryRawCoverageIncomplete
    );
    assert!(!mismatch.did_canonical_apply());
    assert_eq!(
        mismatch
            .candidate_integrity_signal
            .as_ref()
            .map(|signal| signal.outcome),
        Some(CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete)
    );
    assert_eq!(mismatch_ledger.snapshot().canonical_mutation_count, 1);

    let exceeded_sig = signature(185);
    let mut exceeded_ledger = PumpObservationLedgerV1::default();
    assert!(exceeded_ledger
        .observe(
            primary_raw(locator(exceeded_sig, 0), claims.clone(), 186, Some(1),),
            1,
        )
        .observation_decision
        .did_canonical_apply());
    let exceeded = exceeded_ledger
        .observe(
            primary_raw(locator(exceeded_sig, 1), claims.clone(), 187, Some(1)),
            2,
        )
        .observation_decision;
    assert_eq!(
        exceeded.classification,
        PumpObservationClassificationV1::PrimaryRawCoverageIncomplete
    );
    assert!(!exceeded.did_canonical_apply());
    assert_eq!(exceeded_ledger.snapshot().canonical_mutation_count, 1);

    let consistent_sig = signature(188);
    let mut consistent_ledger = PumpObservationLedgerV1::default();
    for ordinal in 0..2 {
        assert!(consistent_ledger
            .observe(
                primary_raw(
                    locator(consistent_sig, ordinal),
                    claims.clone(),
                    189 + ordinal as u8,
                    Some(2),
                ),
                u64::from(ordinal),
            )
            .observation_decision
            .did_canonical_apply());
    }
    assert_eq!(consistent_ledger.snapshot().canonical_mutation_count, 2);
}

#[test]
fn unknown_transaction_inventory_neither_fabricates_singleton_nor_blocks_mutations() {
    let sig = signature(191);
    let claims = complete_claims(key(75), key(76));
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(4, 4, 4, 4)).expect("valid config");
    assert!(ledger
        .observe(primary_raw(locator(sig, 0), claims.clone(), 192, None), 0,)
        .observation_decision
        .did_canonical_apply());
    ledger.observe(
        parsed_nln(sig, None, claims.clone(), "nln-unknown-inventory", 193),
        1,
    );
    let finalized = ledger.finalize_expired(11);
    assert_eq!(finalized.len(), 1);
    assert_eq!(
        finalized[0].classification,
        PumpObservationClassificationV1::AmbiguousParsedWitness,
        "Unknown inventory cannot be coerced to the singleton value one"
    );

    assert!(ledger
        .observe(primary_raw(locator(sig, 1), claims, 194, None), 12)
        .observation_decision
        .did_canonical_apply());
    assert_eq!(ledger.snapshot().canonical_mutation_count, 2);
}

#[test]
fn partial_primary_claims_apply_structurally_without_fabricated_candidate_identity() {
    let sig = signature(163);
    let loc = locator(sig, 0);
    let mut partial_claims = complete_claims(key(61), key(62));
    partial_claims.curve = None;
    let mut ledger = PumpObservationLedgerV1::default();
    let result = ledger
        .observe(primary_raw(loc, partial_claims, 164, Some(1)), 1)
        .observation_decision;

    assert_eq!(
        result.classification,
        PumpObservationClassificationV1::PrimaryCanonicalApplied
    );
    assert!(result.did_canonical_apply());
    assert_eq!(
        result
            .canonical_mutation
            .as_ref()
            .expect("structural canonical mutation")
            .claims
            .curve,
        None,
        "Unknown cannot be fabricated to create candidate identity"
    );
    assert_eq!(
        result.candidate_integrity_signal, None,
        "candidate integrity is not emitted without a raw-derived curve+mint identity"
    );
    assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
}

#[test]
fn tx_index_zero_is_preserved_and_nln_only_never_applies() {
    let sig = signature(78);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(35), key(36));
    let mut ledger = PumpObservationLedgerV1::default();
    let raw = ledger
        .observe(primary_raw(loc, claims.clone(), 112, Some(1)), 1)
        .observation_decision;
    assert_eq!(
        raw.canonical_mutation
            .as_ref()
            .map(|mutation| mutation.order.tx_index),
        Some(0)
    );

    let mut nln_only =
        PumpObservationLedgerV1::try_new(small_config(2, 2, 2, 2)).expect("valid config");
    let nln_sig = signature(79);
    let nln_decision = nln_only
        .observe(parsed_nln(nln_sig, None, claims, "nln-only", 113), 1)
        .observation_decision;
    assert!(!nln_decision.did_canonical_apply());
    let finalized = nln_only.finalize_expired(11);
    assert_eq!(
        finalized[0].correlation,
        Some(ParsedWitnessCorrelationOutcomeV1::Unmatchable)
    );
    assert_eq!(nln_only.snapshot().canonical_mutation_count, 0);
}

#[test]
fn complete_locator_without_an_exact_target_is_unmatchable_not_ambiguous() {
    let sig = signature(80);
    let claims = complete_claims(key(37), key(38));
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(2, 2, 2, 2)).expect("valid config");
    ledger.observe(
        primary_raw(locator(sig, 0), claims.clone(), 114, Some(1)),
        1,
    );
    ledger.observe(
        parsed_nln(
            sig,
            Some(locator(sig, 1)),
            claims,
            "nln-wrong-exact-locator",
            115,
        ),
        2,
    );

    let finalized = ledger.finalize_expired(12);
    assert_eq!(finalized.len(), 1);
    assert_eq!(
        finalized[0].correlation,
        Some(ParsedWitnessCorrelationOutcomeV1::Unmatchable),
        "a complete locator that has no exact raw target cannot be downgraded to signature ambiguity"
    );
    assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
}

#[test]
fn locatorless_and_nonmatching_complete_witnesses_finalize_independently() {
    let sig = signature(85);
    let claims = complete_claims(key(45), key(46));
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(4, 4, 4, 4)).expect("valid config");
    ledger.observe(
        primary_raw(locator(sig, 0), claims.clone(), 127, Some(2)),
        1,
    );
    ledger.observe(
        primary_raw(locator(sig, 1), claims.clone(), 128, Some(2)),
        2,
    );
    ledger.observe(
        parsed_nln(sig, None, claims.clone(), "nln-locatorless", 129),
        3,
    );
    ledger.observe(
        parsed_nln(
            sig,
            Some(locator(sig, 2)),
            claims,
            "nln-complete-but-unmatched",
            130,
        ),
        3,
    );

    let finalized = ledger.finalize_expired(13);
    assert_eq!(finalized.len(), 2);
    assert_eq!(
        finalized
            .iter()
            .filter(|decision| {
                decision.correlation == Some(ParsedWitnessCorrelationOutcomeV1::Ambiguous)
            })
            .count(),
        1,
        "the locatorless witness remains ambiguous among the two raw mutations"
    );
    assert_eq!(
        finalized
            .iter()
            .filter(|decision| {
                decision.correlation == Some(ParsedWitnessCorrelationOutcomeV1::Unmatchable)
            })
            .count(),
        1,
        "the complete nonmatching locator is unmatchable independently"
    );
}

#[test]
fn staggered_locatorless_witnesses_never_create_a_false_singleton() {
    let sig = signature(86);
    let claims = complete_claims(key(47), key(48));
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(4, 4, 4, 4)).expect("valid config");
    ledger.observe(
        primary_raw(locator(sig, 0), claims.clone(), 131, Some(1)),
        0,
    );
    ledger.observe(parsed_nln(sig, None, claims.clone(), "nln-first", 132), 1);
    ledger.observe(parsed_nln(sig, None, claims, "nln-second", 133), 9);

    let mut finalized = ledger.finalize_expired(11);
    finalized.extend(ledger.finalize_expired(19));
    assert_eq!(finalized.len(), 2);
    assert!(finalized.iter().all(|decision| {
        decision.correlation == Some(ParsedWitnessCorrelationOutcomeV1::Ambiguous)
    }));
    assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
    assert_eq!(ledger.snapshot().pending_witness_count, 0);
}

#[test]
fn secondary_raw_pending_correlation_does_not_use_parsed_nln_outcome() {
    let sig = signature(81);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(39), key(40));
    let mut ledger = PumpObservationLedgerV1::default();
    let secondary = secondary_raw(loc.clone(), claims.clone(), "raw-secondary-first", 116);
    assert_eq!(
        ledger
            .observe(secondary, 1)
            .observation_decision
            .classification,
        PumpObservationClassificationV1::SecondaryWitnessOnly
    );

    let primary = ledger.observe(primary_raw(loc, claims, 117, Some(1)), 2);
    assert!(primary.observation_decision.did_canonical_apply());
    assert_eq!(primary.derived_decisions.len(), 1);
    assert_eq!(
        primary.derived_decisions[0].correlation, None,
        "ParsedWitnessCorrelationOutcomeV1 must not label a secondary raw provider"
    );
    assert_eq!(
        primary.derived_decisions[0].classification,
        PumpObservationClassificationV1::SameMutationAgreement
    );
    assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
}

#[test]
fn contradictory_same_identity_witness_replay_is_order_independent_conflict_evidence() {
    let sig = signature(201);
    let loc = locator(sig, 0);
    let primary_claims = complete_claims(key(81), key(82));
    let witness_a = secondary_raw(
        loc.clone(),
        primary_claims.clone(),
        "raw-secondary-replay",
        202,
    );
    let mut witness_b = witness_a.clone();
    witness_b.claims.token_amount_units = Some(43);
    let primary = primary_raw(loc, primary_claims, 203, Some(1));

    let mut witness_first = PumpObservationLedgerV1::default();
    assert_eq!(
        witness_first
            .observe(witness_a.clone(), 1)
            .observation_decision
            .classification,
        PumpObservationClassificationV1::SecondaryWitnessOnly
    );
    assert_eq!(
        witness_first
            .observe(witness_b.clone(), 2)
            .observation_decision
            .classification,
        PumpObservationClassificationV1::SecondaryWitnessOnly,
        "same captured identity with a contradictory normalization is evidence, not an exact replay"
    );
    let witness_first_primary = witness_first.observe(primary.clone(), 3);
    assert!(witness_first_primary
        .derived_decisions
        .iter()
        .any(|decision| {
            decision.classification == PumpObservationClassificationV1::SourceReconciliationConflict
                && decision
                    .conflict_fields
                    .contains(&PumpMutationConflictFieldV1::TokenAmountUnits)
        }));

    let mut raw_first = PumpObservationLedgerV1::default();
    assert!(raw_first
        .observe(primary, 1)
        .observation_decision
        .did_canonical_apply());
    raw_first.observe(witness_a, 2);
    let raw_first_conflict = raw_first.observe(witness_b, 3).observation_decision;
    assert_eq!(
        raw_first_conflict.classification,
        PumpObservationClassificationV1::SourceReconciliationConflict
    );

    assert_eq!(witness_first.snapshot(), raw_first.snapshot());
    assert_eq!(
        witness_first.retained_conflicts(),
        raw_first.retained_conflicts()
    );
}

#[test]
fn same_identity_unknown_to_concrete_agreement_is_retained_in_both_arrival_orders() {
    let sig = signature(206);
    let loc = locator(sig, 0);
    let primary_claims = complete_claims(key(85), key(86));
    let primary = primary_raw(loc.clone(), primary_claims.clone(), 207, Some(1));
    let mut witness_unknown = secondary_raw(
        loc,
        primary_claims,
        "raw-secondary-normalization-refinement",
        208,
    );
    witness_unknown.claims.token_amount_units = None;
    let mut witness_concrete = witness_unknown.clone();
    witness_concrete.claims.token_amount_units = Some(42);

    let mut raw_first = PumpObservationLedgerV1::default();
    assert!(raw_first
        .observe(primary.clone(), 1)
        .observation_decision
        .did_canonical_apply());
    for (now, witness) in [(2, witness_unknown.clone()), (3, witness_concrete.clone())] {
        let decision = raw_first.observe(witness, now).observation_decision;
        assert_eq!(
            decision.classification,
            PumpObservationClassificationV1::SameMutationAgreement
        );
        assert_eq!(
            decision.provider_agreement,
            PumpProviderAgreementV1::PrimarySecondaryAgreement
        );
        assert!(decision.conflict_fields.is_empty());
    }
    let raw_first_before_replays = raw_first.snapshot();
    assert_eq!(raw_first_before_replays.canonical_mutation_count, 1);
    assert_eq!(raw_first_before_replays.exact_duplicate_count, 0);
    assert_eq!(raw_first_before_replays.conflict_count, 0);

    let mut witness_first = PumpObservationLedgerV1::default();
    for (now, witness) in [(1, witness_unknown.clone()), (2, witness_concrete.clone())] {
        assert_eq!(
            witness_first
                .observe(witness, now)
                .observation_decision
                .classification,
            PumpObservationClassificationV1::SecondaryWitnessOnly
        );
    }
    let resolved = witness_first.observe(primary, 3);
    assert!(resolved.observation_decision.did_canonical_apply());
    assert_eq!(resolved.derived_decisions.len(), 2);
    assert!(resolved.derived_decisions.iter().all(|decision| {
        decision.classification == PumpObservationClassificationV1::SameMutationAgreement
            && decision.provider_agreement == PumpProviderAgreementV1::PrimarySecondaryAgreement
            && decision.conflict_fields.is_empty()
    }));
    assert_eq!(witness_first.snapshot(), raw_first_before_replays);

    for ledger in [&mut raw_first, &mut witness_first] {
        for (now, witness) in [(4, witness_unknown.clone()), (5, witness_concrete.clone())] {
            assert_eq!(
                ledger
                    .observe(witness, now)
                    .observation_decision
                    .classification,
                PumpObservationClassificationV1::ExactDuplicate,
                "each retained full normalization must be independently replayable"
            );
        }
        assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
        assert_eq!(ledger.snapshot().exact_duplicate_count, 2);
        assert_eq!(ledger.snapshot().conflict_count, 0);
    }
    assert_eq!(witness_first.snapshot(), raw_first.snapshot());
}

#[test]
fn same_identity_inventory_divergence_is_typed_and_retained_in_both_arrival_orders() {
    let sig = signature(209);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(87), key(88));
    let primary = primary_raw(loc.clone(), claims.clone(), 210, None);
    let witness_count_one = secondary_raw(loc, claims, "raw-secondary-inventory-divergence", 211);
    let mut witness_count_two = witness_count_one.clone();
    witness_count_two.raw_transaction_mutation_count = Some(2);

    let assert_inventory_conflict = |decision: &ghost_core::PumpObservationLedgerDecisionV1| {
        assert_eq!(
            decision.classification,
            PumpObservationClassificationV1::SourceReconciliationConflict
        );
        assert_eq!(
            decision.provider_agreement,
            PumpProviderAgreementV1::PrimarySecondaryConflict
        );
        assert_eq!(
            decision.conflict_fields,
            vec![PumpMutationConflictFieldV1::RawTransactionMutationCount]
        );
        assert_eq!(
            decision
                .candidate_integrity_signal
                .as_ref()
                .map(|signal| signal.outcome),
            Some(CandidateIntegrityOutcomeV1::SourceReconciliationConflict)
        );
    };

    let mut raw_first = PumpObservationLedgerV1::default();
    assert!(raw_first
        .observe(primary.clone(), 1)
        .observation_decision
        .did_canonical_apply());
    assert_eq!(
        raw_first
            .observe(witness_count_one.clone(), 2)
            .observation_decision
            .classification,
        PumpObservationClassificationV1::SameMutationAgreement
    );
    let raw_first_conflict = raw_first
        .observe(witness_count_two.clone(), 3)
        .observation_decision;
    assert_inventory_conflict(&raw_first_conflict);
    let raw_first_before_replays = raw_first.snapshot();
    assert_eq!(raw_first_before_replays.canonical_mutation_count, 1);
    assert_eq!(raw_first_before_replays.exact_duplicate_count, 0);
    assert_eq!(raw_first_before_replays.conflict_count, 1);

    let mut witness_first = PumpObservationLedgerV1::default();
    for (now, witness) in [
        (1, witness_count_one.clone()),
        (2, witness_count_two.clone()),
    ] {
        assert_eq!(
            witness_first
                .observe(witness, now)
                .observation_decision
                .classification,
            PumpObservationClassificationV1::SecondaryWitnessOnly
        );
    }
    let resolved = witness_first.observe(primary, 3);
    assert!(resolved.observation_decision.did_canonical_apply());
    assert_eq!(resolved.derived_decisions.len(), 2);
    assert!(resolved.derived_decisions.iter().any(|decision| {
        decision.classification == PumpObservationClassificationV1::SameMutationAgreement
    }));
    let witness_first_conflict = resolved
        .derived_decisions
        .iter()
        .find(|decision| {
            decision.classification == PumpObservationClassificationV1::SourceReconciliationConflict
        })
        .expect("inventory divergence must remain typed in witness-first order");
    assert_inventory_conflict(witness_first_conflict);
    assert_eq!(witness_first.snapshot(), raw_first_before_replays);
    assert_eq!(
        witness_first.retained_conflicts(),
        raw_first.retained_conflicts()
    );

    for ledger in [&mut raw_first, &mut witness_first] {
        for (now, witness) in [
            (4, witness_count_one.clone()),
            (5, witness_count_two.clone()),
        ] {
            assert_eq!(
                ledger
                    .observe(witness, now)
                    .observation_decision
                    .classification,
                PumpObservationClassificationV1::ExactDuplicate,
                "each retained inventory normalization must be independently replayable"
            );
        }
        assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
        assert_eq!(ledger.snapshot().exact_duplicate_count, 2);
        assert_eq!(ledger.snapshot().conflict_count, 1);
    }
    assert_eq!(witness_first.snapshot(), raw_first.snapshot());
}

#[test]
fn same_identity_retained_concrete_claim_divergence_is_typed_in_both_arrival_orders() {
    for (index, (case, expected_field)) in CLAIM_CASES.iter().enumerate() {
        let sig = signature(220 + index as u8);
        let loc = locator(sig, 0);
        let mut primary_claims = complete_claims(key(89), key(90));
        clear_claim(*case, &mut primary_claims);
        let witness_a_claims = complete_claims(key(89), key(90));
        let mut witness_b_claims = witness_a_claims.clone();
        make_concrete_difference(*case, &mut witness_b_claims);
        let primary = primary_raw(loc.clone(), primary_claims, 220 + index as u8, Some(1));
        let witness_a = secondary_raw(
            loc,
            witness_a_claims,
            "raw-secondary-retained-claim-divergence",
            240 + index as u8,
        );
        let mut witness_b = witness_a.clone();
        witness_b.claims = witness_b_claims;

        let assert_typed_conflict = |decision: &ghost_core::PumpObservationLedgerDecisionV1| {
            assert_eq!(
                decision.classification,
                PumpObservationClassificationV1::SourceReconciliationConflict,
                "case={case:?}"
            );
            assert_eq!(
                decision.provider_agreement,
                PumpProviderAgreementV1::PrimarySecondaryConflict,
                "case={case:?}"
            );
            assert_eq!(
                decision.conflict_fields,
                vec![*expected_field],
                "case={case:?}"
            );
            if !matches!(case, ClaimCase::Curve | ClaimCase::Mint) {
                assert_eq!(
                    decision
                        .candidate_integrity_signal
                        .as_ref()
                        .map(|signal| signal.outcome),
                    Some(CandidateIntegrityOutcomeV1::SourceReconciliationConflict),
                    "case={case:?}"
                );
            }
        };

        let mut raw_first = PumpObservationLedgerV1::default();
        assert!(raw_first
            .observe(primary.clone(), 1)
            .observation_decision
            .did_canonical_apply());
        assert_eq!(
            raw_first
                .observe(witness_a.clone(), 2)
                .observation_decision
                .classification,
            PumpObservationClassificationV1::SameMutationAgreement,
            "case={case:?}"
        );
        let raw_first_conflict = raw_first.observe(witness_b.clone(), 3).observation_decision;
        assert_typed_conflict(&raw_first_conflict);
        let raw_first_before_replays = raw_first.snapshot();
        assert_eq!(raw_first_before_replays.canonical_mutation_count, 1);
        assert_eq!(raw_first_before_replays.exact_duplicate_count, 0);
        assert_eq!(raw_first_before_replays.conflict_count, 1);

        let mut witness_first = PumpObservationLedgerV1::default();
        for (now, witness) in [(1, witness_a.clone()), (2, witness_b.clone())] {
            assert_eq!(
                witness_first
                    .observe(witness, now)
                    .observation_decision
                    .classification,
                PumpObservationClassificationV1::SecondaryWitnessOnly,
                "case={case:?}"
            );
        }
        let resolved = witness_first.observe(primary, 3);
        assert!(resolved.observation_decision.did_canonical_apply());
        assert_eq!(resolved.derived_decisions.len(), 2);
        let witness_first_conflict = resolved
            .derived_decisions
            .iter()
            .find(|decision| {
                decision.classification
                    == PumpObservationClassificationV1::SourceReconciliationConflict
            })
            .expect("retained concrete divergence must remain typed");
        assert_typed_conflict(witness_first_conflict);
        assert_eq!(witness_first.snapshot(), raw_first_before_replays);
        assert_eq!(
            witness_first.retained_conflicts(),
            raw_first.retained_conflicts(),
            "case={case:?}"
        );

        for ledger in [&mut raw_first, &mut witness_first] {
            for (now, witness) in [(4, witness_a.clone()), (5, witness_b.clone())] {
                assert_eq!(
                    ledger
                        .observe(witness, now)
                        .observation_decision
                        .classification,
                    PumpObservationClassificationV1::ExactDuplicate,
                    "retained variant replay case={case:?}"
                );
            }
            assert_eq!(ledger.snapshot().canonical_mutation_count, 1);
            assert_eq!(ledger.snapshot().exact_duplicate_count, 2);
            assert_eq!(ledger.snapshot().conflict_count, 1);
        }
        assert_eq!(
            witness_first.snapshot(),
            raw_first.snapshot(),
            "case={case:?}"
        );
    }
}

#[test]
fn secondary_witness_expiry_retains_bounded_identity_and_first_overflow_evidence() {
    let sig = signature(204);
    let loc = locator(sig, 0);
    let claims = complete_claims(key(83), key(84));
    let witness = secondary_raw(loc, claims, "raw-secondary-expiry-audit", 205);
    let mut ledger =
        PumpObservationLedgerV1::try_new(small_config(2, 1, 2, 2)).expect("valid config");

    ledger.observe(witness.clone(), 0);
    let first_expiry = ledger.finalize_expired(10);
    assert_eq!(first_expiry.len(), 1);
    assert_eq!(
        first_expiry[0].classification,
        PumpObservationClassificationV1::SecondaryWitnessExpired
    );
    assert_eq!(
        first_expiry[0].expired_witness_observation.as_ref(),
        Some(&witness)
    );
    assert_eq!(ledger.retained_expired_witnesses(), &[witness.clone()]);

    let replay = ledger.observe(witness.clone(), 11).observation_decision;
    assert_eq!(
        replay.classification,
        PumpObservationClassificationV1::SecondaryWitnessOnly,
        "expiry releases correlation capacity without erasing its retained audit identity"
    );
    let second_expiry = ledger.finalize_expired(21);
    assert_eq!(second_expiry.len(), 1);
    assert_eq!(
        second_expiry[0].expired_witness_observation.as_ref(),
        Some(&witness)
    );
    assert!(!second_expiry[0].evidence_complete);

    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.retained_expired_witness_count, 1);
    assert_eq!(snapshot.expired_witness_audit_overflow_count, 1);
    assert_eq!(
        snapshot.first_rejected_expired_witness.as_ref(),
        Some(&witness)
    );
    assert_eq!(
        snapshot
            .first_evidence_overflow
            .as_ref()
            .map(|overflow| overflow.lane),
        Some(PumpObservationEvidenceLaneV1::ExpiredWitnessAudit)
    );
}
