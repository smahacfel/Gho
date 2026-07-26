//! Frozen differential corpus gate for PR1D Pump Observation Ledger.
//!
//! This test intentionally does not call the not-yet-implemented production
//! ledger API. It freezes the pre-implementation schema, scenario inventory,
//! expected outcomes, and exact fixture bytes. Executable replay is added
//! after the ledger exists without rewriting this v1 fixture.

#![allow(dead_code)]

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

const CORPUS_BYTES: &[u8] = include_bytes!(
    "fixtures/pump_observation_ledger_v1/pump_observation_differential_corpus_v1.jsonl"
);
const CORPUS_BLAKE3_HEX: &str = "833de2bd384c964712f2e7127f9bc1db57745644633c1c66facef540cdf4c2a4";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusScenario {
    schema_version: u32,
    scenario_id: String,
    description: String,
    #[serde(default)]
    capacity_profile: Option<CapacityProfile>,
    #[serde(default)]
    account_handoff: Option<AccountHandoff>,
    #[serde(default)]
    lifecycle_matrix: Vec<LifecycleEntry>,
    observations: Vec<CorpusObservation>,
    expected: ExpectedScenario,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapacityProfile {
    max_pending_witnesses_per_signature: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountHandoff {
    arbiter_classification: String,
    registry_outcome: String,
    canonical_account_apply: bool,
    strategic_verdict_emitted: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleEntry {
    phase: String,
    expected_reaction: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusObservation {
    observation_id: String,
    source_family: String,
    provider_role: String,
    mutation_family: String,
    signature: String,
    locator: Option<FixtureLocator>,
    canonical_order: Option<FixtureOrder>,
    raw_transaction_pump_mutation_count: Option<u32>,
    claims: FixtureClaims,
    provenance: Option<FixtureProvenance>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct FixtureLocator {
    program_id: String,
    signature: String,
    outer_instruction_index: u32,
    inner_instruction_path: Vec<u32>,
    semantic_event_ordinal: u32,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct FixtureOrder {
    slot: u64,
    tx_index: u32,
    outer_instruction_index: u32,
    inner_instruction_path: Vec<u32>,
    semantic_event_ordinal: u32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FixtureClaims {
    curve: Option<String>,
    mint: Option<String>,
    route_variant: Option<String>,
    side: Option<String>,
    success: Option<bool>,
    token_amount_units: Option<u64>,
    instruction_limit: Option<InstructionLimit>,
    reported_curve_quote_lamports: Option<u64>,
    reported_wallet_delta_lamports: Option<u64>,
    reported_fee_breakdown: Option<Vec<FeeCharge>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstructionLimit {
    kind: String,
    amount: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FeeCharge {
    recipient: String,
    lamports: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureProvenance {
    source_id: String,
    provider_id: String,
    schema_id: String,
    payload_hash_blake3: String,
    received_at_monotonic_ns: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedScenario {
    decisions: Vec<ExpectedDecision>,
    finalize: Option<ExpectedFinalize>,
    final_state: ExpectedFinalState,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedDecision {
    observation_id: String,
    classification: String,
    canonical_apply: bool,
    correlation_outcome: String,
    provider_agreement: String,
    conflict_fields: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedFinalize {
    classification: String,
    correlation_outcome: String,
    provider_agreement: String,
    conflict_fields: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ExpectedFinalState {
    canonical_mutation_count: u64,
    nln_canonical_event_count: u64,
    correlation_outcome: String,
    candidate_integrity: String,
    evidence_complete: bool,
    raw_waited_for_nln: bool,
}

#[test]
fn pump_observation_differential_corpus_v1_is_frozen_complete_and_self_consistent() {
    let actual_digest = blake3::hash(CORPUS_BYTES).to_hex().to_string();
    assert_eq!(
        actual_digest, CORPUS_BLAKE3_HEX,
        "the PR1D hard-gate corpus changed; create a new corpus version instead of rewriting v1"
    );
    assert!(
        CORPUS_BYTES.ends_with(b"\n"),
        "the frozen JSONL must end with exactly one record terminator"
    );
    assert!(
        !CORPUS_BYTES.contains(&b'\r'),
        "the frozen JSONL must use LF, not CRLF"
    );

    let corpus_text = std::str::from_utf8(CORPUS_BYTES).expect("corpus must be UTF-8 JSONL");
    let lines: Vec<_> = corpus_text.lines().collect();
    assert_eq!(lines.len(), 33, "scenario count is part of the v1 contract");
    assert!(lines.iter().all(|line| {
        !line.is_empty() && *line == line.trim() && line.starts_with('{') && line.ends_with('}')
    }));

    let scenarios: Vec<CorpusScenario> = lines
        .iter()
        .map(|line| {
            serde_json::from_str(line).expect("every corpus line must satisfy the strict v1 schema")
        })
        .collect();
    validate_inventory(&scenarios);
    validate_common_invariants(&scenarios);
    validate_material_claim_matrix(&scenarios);
    validate_arrival_order_symmetry(&scenarios);
    validate_scenario_specific_contracts(&scenarios);
}

fn validate_inventory(scenarios: &[CorpusScenario]) {
    let actual: BTreeSet<_> = scenarios
        .iter()
        .map(|scenario| scenario.scenario_id.as_str())
        .collect();
    let required = BTreeSet::from([
        "account_provider_conflict_handoff",
        "create_and_initial_buy_same_signature",
        "cross_source_payload_hash_difference_agreement",
        "exact_locator_among_multiple_raw",
        "exact_nln_then_raw_agreement",
        "exact_raw_then_nln_agreement",
        "lifecycle_conflict_matrix",
        "locatorless_nln_ambiguous",
        "material_conflict_curve",
        "material_conflict_instruction_limit",
        "material_conflict_mint",
        "material_conflict_reported_curve_quote_lamports",
        "material_conflict_reported_fee_breakdown",
        "material_conflict_reported_wallet_delta_lamports",
        "material_conflict_route_variant",
        "material_conflict_side",
        "material_conflict_success",
        "material_conflict_token_amount_units",
        "missing_primary_order_fail_closed",
        "multiple_raw_mutations_same_signature",
        "nln_only_expiry_unmatchable",
        "primary_raw_missing_locator_fail_closed",
        "primary_raw_missing_provenance_fail_closed",
        "raw_only_immediate",
        "reconnect_raw_exact_duplicate",
        "second_locatorless_nln_prevents_singleton",
        "secondary_raw_different_payload_conflict",
        "secondary_raw_same_payload_agreement",
        "singleton_nln_then_raw_agreement",
        "singleton_raw_then_nln_agreement",
        "tx_index_zero_preserved",
        "unknown_claim_vs_concrete",
        "witness_saturation_no_primary_veto",
    ]);
    assert_eq!(actual, required, "hard-gate scenario inventory drifted");
}

fn validate_common_invariants(scenarios: &[CorpusScenario]) {
    const SOURCE_FAMILIES: &[&str] = &["raw_yellowstone", "parsed_nln"];
    const PROVIDER_ROLES: &[&str] = &["primary_authority", "secondary_witness"];
    const MUTATION_FAMILIES: &[&str] = &["initialize_pool", "trade"];
    const CLASSIFICATIONS: &[&str] = &[
        "canonical_primary_applied",
        "evidence_capacity_exceeded",
        "exact_duplicate",
        "pending_witness_recorded",
        "primary_raw_coverage_incomplete",
        "secondary_raw_witness_correlated",
        "source_reconciliation_conflict",
        "witness_correlated",
    ];
    const CORRELATIONS: &[&str] = &[
        "ambiguous",
        "exact_replay_duplicate",
        "exact_structural_match",
        "not_observed",
        "pending",
        "unique_signature_singleton_match",
        "unmatchable",
    ];
    const AGREEMENTS: &[&str] = &[
        "no_conflict_with_unknown",
        "not_observed",
        "primary_secondary_agreement",
        "primary_secondary_conflict",
        "witness_only",
    ];
    const INTEGRITY: &[&str] = &[
        "account_provider_conflict",
        "primary_raw_coverage_incomplete",
        "ready",
        "source_reconciliation_conflict",
    ];

    for scenario in scenarios {
        assert_eq!(scenario.schema_version, 1);
        assert!(!scenario.scenario_id.is_empty());
        assert!(!scenario.description.is_empty());
        assert!(!scenario.observations.is_empty());
        assert_eq!(
            scenario.observations.len(),
            scenario.expected.decisions.len(),
            "one typed decision is required per accepted observation: {}",
            scenario.scenario_id
        );
        assert!(INTEGRITY.contains(&scenario.expected.final_state.candidate_integrity.as_str()));
        assert_eq!(
            scenario.expected.final_state.nln_canonical_event_count, 0,
            "parsed NLN must never create a canonical event: {}",
            scenario.scenario_id
        );
        assert!(
            !scenario.expected.final_state.raw_waited_for_nln,
            "raw must never wait for NLN: {}",
            scenario.scenario_id
        );

        let mut observation_ids = BTreeSet::new();
        let mut canonical_apply_count = 0_u64;
        for (observation, decision) in scenario
            .observations
            .iter()
            .zip(scenario.expected.decisions.iter())
        {
            assert!(observation_ids.insert(observation.observation_id.as_str()));
            assert_eq!(decision.observation_id, observation.observation_id);
            assert!(SOURCE_FAMILIES.contains(&observation.source_family.as_str()));
            assert!(PROVIDER_ROLES.contains(&observation.provider_role.as_str()));
            assert!(MUTATION_FAMILIES.contains(&observation.mutation_family.as_str()));
            assert!(CLASSIFICATIONS.contains(&decision.classification.as_str()));
            assert!(CORRELATIONS.contains(&decision.correlation_outcome.as_str()));
            assert!(AGREEMENTS.contains(&decision.provider_agreement.as_str()));
            assert!(!observation.signature.is_empty());

            if let Some(locator) = &observation.locator {
                assert_eq!(locator.signature, observation.signature);
                assert!(!locator.program_id.is_empty());
            }
            if let (Some(locator), Some(order)) =
                (&observation.locator, &observation.canonical_order)
            {
                assert_eq!(
                    order.outer_instruction_index,
                    locator.outer_instruction_index
                );
                assert_eq!(order.inner_instruction_path, locator.inner_instruction_path);
                assert_eq!(order.semantic_event_ordinal, locator.semantic_event_ordinal);
            }
            if let Some(provenance) = &observation.provenance {
                assert!(!provenance.source_id.is_empty());
                assert!(!provenance.provider_id.is_empty());
                assert!(!provenance.schema_id.is_empty());
                assert_lower_hex_digest(&provenance.payload_hash_blake3);
            }

            if observation.source_family == "parsed_nln" {
                assert_eq!(observation.provider_role, "secondary_witness");
                assert!(observation.canonical_order.is_none());
                assert!(observation.raw_transaction_pump_mutation_count.is_none());
                assert!(
                    !decision.canonical_apply,
                    "parsed NLN cannot acquire canonical authority: {}",
                    scenario.scenario_id
                );
            } else {
                assert!(
                    observation.raw_transaction_pump_mutation_count.is_some(),
                    "raw inventory count must be explicit: {}",
                    scenario.scenario_id
                );
            }
            if observation.provider_role == "secondary_witness" {
                assert!(
                    !decision.canonical_apply,
                    "secondary witness cannot canonical-apply: {}",
                    scenario.scenario_id
                );
            }
            if decision.canonical_apply {
                canonical_apply_count = canonical_apply_count.saturating_add(1);
                assert_eq!(observation.source_family, "raw_yellowstone");
                assert_eq!(observation.provider_role, "primary_authority");
                assert!(observation.locator.is_some());
                assert!(observation.canonical_order.is_some());
                assert!(observation.provenance.is_some());
            }
        }
        assert_eq!(
            canonical_apply_count, scenario.expected.final_state.canonical_mutation_count,
            "canonical mutation count must equal canonical_apply decisions: {}",
            scenario.scenario_id
        );

        if let Some(finalize) = &scenario.expected.finalize {
            assert_eq!(finalize.classification, "correlation_finalized");
            assert!(CORRELATIONS.contains(&finalize.correlation_outcome.as_str()));
            assert!(AGREEMENTS.contains(&finalize.provider_agreement.as_str()));
            assert_eq!(
                finalize.correlation_outcome,
                scenario.expected.final_state.correlation_outcome
            );
        }
    }
}

fn validate_material_claim_matrix(scenarios: &[CorpusScenario]) {
    let required_fields = BTreeSet::from([
        "curve",
        "instruction_limit",
        "mint",
        "reported_curve_quote_lamports",
        "reported_fee_breakdown",
        "reported_wallet_delta_lamports",
        "route_variant",
        "side",
        "success",
        "token_amount_units",
    ]);
    let mut actual_fields = BTreeSet::new();

    for scenario in scenarios
        .iter()
        .filter(|scenario| scenario.scenario_id.starts_with("material_conflict_"))
    {
        assert_eq!(scenario.expected.final_state.canonical_mutation_count, 1);
        assert_eq!(
            scenario.expected.final_state.candidate_integrity,
            "source_reconciliation_conflict"
        );
        let conflict = scenario
            .expected
            .decisions
            .iter()
            .find(|decision| decision.classification == "source_reconciliation_conflict")
            .expect("material-conflict scenario must preserve a conflict decision");
        assert_eq!(conflict.conflict_fields.len(), 1);
        let field = conflict.conflict_fields[0].as_str();
        assert_eq!(
            scenario
                .scenario_id
                .strip_prefix("material_conflict_")
                .expect("prefix checked"),
            field
        );
        actual_fields.insert(field);
    }
    assert_eq!(
        actual_fields, required_fields,
        "every material PumpMutationClaimsV1 field must have a concrete-difference conflict fixture"
    );
}

fn validate_arrival_order_symmetry(scenarios: &[CorpusScenario]) {
    let by_id: BTreeMap<_, _> = scenarios
        .iter()
        .map(|scenario| (scenario.scenario_id.as_str(), scenario))
        .collect();
    assert_eq!(
        by_id["exact_raw_then_nln_agreement"].expected.final_state,
        by_id["exact_nln_then_raw_agreement"].expected.final_state
    );
    assert_eq!(
        by_id["singleton_raw_then_nln_agreement"]
            .expected
            .final_state,
        by_id["singleton_nln_then_raw_agreement"]
            .expected
            .final_state
    );
}

fn validate_scenario_specific_contracts(scenarios: &[CorpusScenario]) {
    let by_id: BTreeMap<_, _> = scenarios
        .iter()
        .map(|scenario| (scenario.scenario_id.as_str(), scenario))
        .collect();

    let multi = by_id["multiple_raw_mutations_same_signature"];
    assert_eq!(multi.expected.final_state.canonical_mutation_count, 2);
    assert_eq!(
        multi
            .observations
            .iter()
            .map(|observation| observation.signature.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        1
    );
    assert_eq!(
        multi
            .observations
            .iter()
            .map(|observation| observation.locator.as_ref().expect("raw locator"))
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );

    let unknown = by_id["unknown_claim_vs_concrete"];
    assert!(unknown.observations[0].claims.token_amount_units.is_some());
    assert!(unknown.observations[1].claims.token_amount_units.is_none());
    assert!(unknown.expected.decisions[1].conflict_fields.is_empty());
    assert_eq!(unknown.expected.final_state.candidate_integrity, "ready");

    let cross_hash = by_id["cross_source_payload_hash_difference_agreement"];
    assert_ne!(
        cross_hash.observations[0]
            .provenance
            .as_ref()
            .expect("raw provenance")
            .payload_hash_blake3,
        cross_hash.observations[1]
            .provenance
            .as_ref()
            .expect("NLN provenance")
            .payload_hash_blake3
    );
    assert_eq!(
        cross_hash.expected.decisions[1].provider_agreement,
        "primary_secondary_agreement"
    );

    let reconnect = by_id["reconnect_raw_exact_duplicate"];
    assert_eq!(
        reconnect.observations[0].locator,
        reconnect.observations[1].locator
    );
    assert_eq!(
        reconnect.observations[0]
            .provenance
            .as_ref()
            .expect("provenance")
            .payload_hash_blake3,
        reconnect.observations[1]
            .provenance
            .as_ref()
            .expect("provenance")
            .payload_hash_blake3
    );
    assert_ne!(
        reconnect.observations[0]
            .provenance
            .as_ref()
            .expect("provenance")
            .received_at_monotonic_ns,
        reconnect.observations[1]
            .provenance
            .as_ref()
            .expect("provenance")
            .received_at_monotonic_ns
    );

    assert_eq!(
        by_id["tx_index_zero_preserved"].observations[0]
            .canonical_order
            .as_ref()
            .expect("order must be present")
            .tx_index,
        0
    );
    assert!(by_id["missing_primary_order_fail_closed"].observations[0]
        .canonical_order
        .is_none());
    assert!(
        by_id["primary_raw_missing_locator_fail_closed"].observations[0]
            .locator
            .is_none()
    );
    assert!(
        by_id["primary_raw_missing_provenance_fail_closed"].observations[0]
            .provenance
            .is_none()
    );

    let saturation = by_id["witness_saturation_no_primary_veto"];
    assert_eq!(
        saturation
            .capacity_profile
            .as_ref()
            .expect("capacity profile")
            .max_pending_witnesses_per_signature,
        2
    );
    assert_eq!(
        saturation
            .observations
            .iter()
            .filter(|observation| observation.source_family == "parsed_nln")
            .count(),
        3
    );
    assert!(saturation
        .expected
        .decisions
        .iter()
        .any(|decision| { decision.classification == "evidence_capacity_exceeded" }));
    assert!(saturation
        .expected
        .decisions
        .last()
        .is_some_and(|decision| {
            decision.classification == "canonical_primary_applied" && decision.canonical_apply
        }));
    assert!(!saturation.expected.final_state.evidence_complete);

    let create_buy = by_id["create_and_initial_buy_same_signature"];
    assert_eq!(create_buy.expected.final_state.canonical_mutation_count, 2);
    assert_eq!(
        create_buy
            .observations
            .iter()
            .map(|observation| observation.mutation_family.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["initialize_pool", "trade"])
    );
    assert_eq!(
        create_buy
            .observations
            .iter()
            .map(|observation| observation.signature.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        1
    );

    let account = by_id["account_provider_conflict_handoff"]
        .account_handoff
        .as_ref()
        .expect("account conflict handoff metadata");
    assert_eq!(
        account.arbiter_classification,
        "same_version_different_hash_conflict"
    );
    assert_eq!(account.registry_outcome, "account_provider_conflict");
    assert!(!account.canonical_account_apply);
    assert!(!account.strategic_verdict_emitted);

    let lifecycle: BTreeMap<_, _> = by_id["lifecycle_conflict_matrix"]
        .lifecycle_matrix
        .iter()
        .map(|entry| (entry.phase.as_str(), entry.expected_reaction.as_str()))
        .collect();
    assert_eq!(
        lifecycle,
        BTreeMap::from([
            (
                "confirmed_open_position",
                "raw_authority_nln_quarantine_protective_exit_active",
            ),
            (
                "evaluation_running",
                "technical_abort_no_terminal_publication",
            ),
            ("mfs_materialized", "interrupt_no_policy_publication"),
            ("pre_mfs", "technical_abort_zero_mfs_zero_gatekeeper",),
            (
                "submit_started",
                "reconciliation_required_unknown_no_fake_cancel",
            ),
            (
                "terminal_buy_not_submitted",
                "cancel_intent_zero_sender_release_lease",
            ),
            ("terminal_reject", "immutable_verdict_plus_audit_marker",),
            ("terminal_timeout", "immutable_verdict_plus_audit_marker",),
        ])
    );

    let two_nln = by_id["second_locatorless_nln_prevents_singleton"];
    assert_eq!(
        two_nln
            .observations
            .iter()
            .filter(|observation| observation.source_family == "parsed_nln")
            .count(),
        2
    );
    assert_eq!(
        two_nln.expected.final_state.correlation_outcome,
        "ambiguous"
    );
}

fn assert_lower_hex_digest(value: &str) {
    assert_eq!(value.len(), 64, "BLAKE3 digests must contain 32 bytes");
    assert!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "BLAKE3 digests must be lower-case hexadecimal"
    );
}
