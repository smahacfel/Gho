//! Frozen differential corpus gates for the production PR1D Pump Observation
//! Ledger.
//!
//! V1 bytes remain immutable. The first test guards their schema and digest;
//! the executable replay test separately adapts every frozen observation into
//! the public production ledger and compares the real decisions with the
//! frozen outcomes.

#![allow(dead_code)]

use ghost_core::{
    CandidateIntegrityOutcomeV1, CanonicalPumpOrderKeyV1, ObservationProvenanceV1,
    ObservationSourceFamilyV1, ObservedPumpMutationV1, ParsedWitnessCorrelationOutcomeV1,
    ProgramFeeCharge, PumpInstructionLimitV1, PumpMutationClaimsV1, PumpMutationConflictFieldV1,
    PumpMutationFamilyV1, PumpObservationClassificationV1, PumpObservationLedgerConfigV1,
    PumpObservationLedgerDecisionV1, PumpObservationLedgerV1, PumpProviderAgreementV1,
    PumpRouteVariant, PumpTradeSideV1, RawProviderRoleV1, RawPumpMutationLocatorV1,
};
use serde::Deserialize;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use std::collections::{BTreeMap, BTreeSet};

const CORPUS_BYTES: &[u8] = include_bytes!(
    "fixtures/pump_observation_ledger_v1/pump_observation_differential_corpus_v1.jsonl"
);
const CORPUS_BLAKE3_HEX: &str = "833de2bd384c964712f2e7127f9bc1db57745644633c1c66facef540cdf4c2a4";
const CORPUS_V2_BYTES: &[u8] = include_bytes!(
    "fixtures/pump_observation_ledger_v2/pump_observation_differential_corpus_v2.jsonl"
);
const CORPUS_V2_BLAKE3_HEX: &str =
    "c81d7b4f0cc3792c2bb2c4e71bfd0634fcfdd69723758d741ee2405770603415";

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
    error_code: Option<String>,
    token_amount_units: Option<u64>,
    instruction_limit: Option<InstructionLimit>,
    reported_curve_quote_lamports: Option<u64>,
    reported_wallet_delta_lamports: Option<u64>,
    reported_fee_breakdown: Option<Vec<FeeCharge>>,
    reported_post_state_hash_blake3: Option<String>,
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

#[test]
fn pump_observation_differential_corpus_v1_replays_through_production_ledger() {
    for scenario in parse_scenarios(CORPUS_BYTES) {
        replay_scenario_through_production_ledger(&scenario);
    }
}

#[test]
fn pump_observation_differential_corpus_v2_replays_new_claims_and_expiry_audit() {
    assert_eq!(
        blake3::hash(CORPUS_V2_BYTES).to_hex().to_string(),
        CORPUS_V2_BLAKE3_HEX,
        "the PR1D v2 corpus changed; create a new corpus version instead of rewriting v2"
    );
    let scenarios = parse_scenarios(CORPUS_V2_BYTES);
    assert_eq!(scenarios.len(), 3);
    assert!(scenarios
        .iter()
        .all(|scenario| scenario.schema_version == 2));
    assert_eq!(
        scenarios
            .iter()
            .map(|scenario| scenario.scenario_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "material_conflict_error_code",
            "material_conflict_reported_post_state_hash_blake3",
            "secondary_witness_expiry_identity_audit",
        ])
    );

    for scenario in scenarios
        .iter()
        .filter(|scenario| scenario.expected.finalize.is_none())
    {
        replay_scenario_through_production_ledger(scenario);
    }

    let expiry = scenarios
        .iter()
        .find(|scenario| scenario.scenario_id == "secondary_witness_expiry_identity_audit")
        .expect("v2 expiry scenario");
    let observation = fixture_observation(&expiry.observations[0], 0);
    let mut ledger = PumpObservationLedgerV1::try_new(PumpObservationLedgerConfigV1 {
        correlation_window_ns: 10,
        max_pending_witnesses: 1,
        ..PumpObservationLedgerConfigV1::default()
    })
    .expect("valid v2 expiry config");
    let observed = ledger.observe(observation.clone(), 30);
    assert_expected_observation_result(
        expiry,
        &expiry.observations[0],
        &expiry.expected.decisions[0],
        &observed.observation_decision,
        &observed.derived_decisions,
        ledger.snapshot().pending_witness_count,
    );
    let finalized = ledger.finalize_expired(40);
    assert_eq!(finalized.len(), 1);
    assert_eq!(
        finalized[0].classification,
        PumpObservationClassificationV1::SecondaryWitnessExpired
    );
    assert_eq!(
        finalized[0].expired_witness_observation.as_ref(),
        Some(&observation)
    );
    assert_eq!(ledger.retained_expired_witnesses(), &[observation]);
    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.canonical_mutation_count, 0);
    assert_eq!(snapshot.pending_witness_count, 0);
    assert_eq!(snapshot.retained_expired_witness_count, 1);
    assert!(snapshot.witness_evidence_complete);
}

fn parse_scenarios(bytes: &[u8]) -> Vec<CorpusScenario> {
    std::str::from_utf8(bytes)
        .expect("corpus must be UTF-8 JSONL")
        .lines()
        .map(|line| serde_json::from_str(line).expect("strict corpus record"))
        .collect()
}

fn replay_scenario_through_production_ledger(scenario: &CorpusScenario) {
    let mut config = PumpObservationLedgerConfigV1::default();
    if let Some(profile) = &scenario.capacity_profile {
        config.max_pending_witnesses = profile.max_pending_witnesses_per_signature;
    }
    let correlation_window_ns = config.correlation_window_ns;
    let mut ledger = PumpObservationLedgerV1::try_new(config).expect("valid corpus capacity");
    let mut last_correlation = "not_observed";
    let mut integrity = CandidateIntegrityOutcomeV1::Ready;
    let mut max_observation_time = 0_u64;

    for (index, (fixture, expected)) in scenario
        .observations
        .iter()
        .zip(&scenario.expected.decisions)
        .enumerate()
    {
        let observation = fixture_observation(fixture, index);
        let now = observation.provenance.received_at_monotonic_ns;
        max_observation_time = max_observation_time.max(now);
        let result = ledger.observe(observation, now);
        let mut decisions = Vec::with_capacity(1 + result.derived_decisions.len());
        decisions.push(&result.observation_decision);
        decisions.extend(result.derived_decisions.iter());

        assert_expected_observation_result(
            scenario,
            fixture,
            expected,
            &result.observation_decision,
            &result.derived_decisions,
            ledger.snapshot().pending_witness_count,
        );
        update_replay_outcomes(&decisions, &mut last_correlation, &mut integrity);
    }

    let finalized = ledger.finalize_expired(
        max_observation_time
            .saturating_add(correlation_window_ns)
            .saturating_add(1),
    );
    if let Some(expected) = &scenario.expected.finalize {
        assert!(
            !finalized.is_empty(),
            "expected a production finalization decision: {}",
            scenario.scenario_id
        );
        for decision in &finalized {
            assert_expected_finalize(scenario, expected, decision);
        }
    } else {
        assert!(
            finalized.is_empty(),
            "unexpected production finalization decisions in {}: {finalized:?}",
            scenario.scenario_id
        );
    }
    update_replay_outcomes(
        &finalized.iter().collect::<Vec<_>>(),
        &mut last_correlation,
        &mut integrity,
    );

    let snapshot = ledger.snapshot();
    assert_eq!(
        snapshot.canonical_mutation_count, scenario.expected.final_state.canonical_mutation_count,
        "production canonical count drifted: {}",
        scenario.scenario_id
    );
    assert_eq!(
        snapshot.primary_evidence_complete && snapshot.witness_evidence_complete,
        scenario.expected.final_state.evidence_complete,
        "production evidence completeness drifted: {}",
        scenario.scenario_id
    );
    assert_eq!(
        last_correlation, scenario.expected.final_state.correlation_outcome,
        "production correlation outcome drifted: {}",
        scenario.scenario_id
    );

    let actual_integrity = scenario
        .account_handoff
        .as_ref()
        .map(|handoff| handoff.registry_outcome.as_str())
        .unwrap_or_else(|| candidate_integrity_label(integrity));
    assert_eq!(
        actual_integrity, scenario.expected.final_state.candidate_integrity,
        "production CandidateIntegrity signal drifted: {}",
        scenario.scenario_id
    );
}

fn fixture_observation(fixture: &CorpusObservation, index: usize) -> ObservedPumpMutationV1 {
    let source_family = match fixture.source_family.as_str() {
        "raw_yellowstone" => ObservationSourceFamilyV1::RawYellowstone,
        "parsed_nln" => ObservationSourceFamilyV1::ParsedNln,
        other => panic!("unknown fixture source family {other}"),
    };
    let signature = fixture_signature(&fixture.signature);
    let provenance = fixture
        .provenance
        .as_ref()
        .map(|provenance| ObservationProvenanceV1 {
            source_family,
            source_id: provenance.source_id.clone(),
            provider_id: provenance.provider_id.clone(),
            schema_id: provenance.schema_id.clone(),
            payload_hash_blake3: parse_digest(&provenance.payload_hash_blake3),
            received_at_monotonic_ns: provenance.received_at_monotonic_ns,
        })
        .unwrap_or(ObservationProvenanceV1 {
            source_family,
            source_id: String::new(),
            provider_id: String::new(),
            schema_id: String::new(),
            payload_hash_blake3: [0; 32],
            received_at_monotonic_ns: index as u64,
        });
    let locator_hint = fixture
        .locator
        .as_ref()
        .map(|locator| RawPumpMutationLocatorV1 {
            program_id: fixture_pubkey(&locator.program_id),
            signature,
            outer_instruction_index: u16::try_from(locator.outer_instruction_index)
                .expect("fixture outer index fits production contract"),
            inner_instruction_path: locator
                .inner_instruction_path
                .iter()
                .map(|index| u16::try_from(*index).expect("fixture path fits production contract"))
                .collect(),
            semantic_event_ordinal: locator.semantic_event_ordinal,
        });
    let canonical_order = fixture
        .canonical_order
        .as_ref()
        .map(|order| CanonicalPumpOrderKeyV1 {
            slot: order.slot,
            tx_index: order.tx_index,
            outer_instruction_index: u16::try_from(order.outer_instruction_index)
                .expect("fixture outer index fits production contract"),
            inner_instruction_path: order
                .inner_instruction_path
                .iter()
                .map(|index| u16::try_from(*index).expect("fixture path fits production contract"))
                .collect(),
            semantic_event_ordinal: order.semantic_event_ordinal,
        });

    ObservedPumpMutationV1 {
        mutation_family: match fixture.mutation_family.as_str() {
            "initialize_pool" => PumpMutationFamilyV1::InitializePool,
            "trade" => PumpMutationFamilyV1::Trade,
            other => panic!("unknown fixture mutation family {other}"),
        },
        signature,
        locator_hint,
        canonical_order,
        raw_transaction_mutation_count: fixture.raw_transaction_pump_mutation_count,
        claims: fixture_claims(&fixture.claims),
        raw_provider_role: match source_family {
            ObservationSourceFamilyV1::ParsedNln => None,
            ObservationSourceFamilyV1::RawYellowstone => {
                Some(match fixture.provider_role.as_str() {
                    "primary_authority" => RawProviderRoleV1::PrimaryAuthority,
                    "secondary_witness" => RawProviderRoleV1::SecondaryWitness,
                    other => panic!("unknown fixture provider role {other}"),
                })
            }
        },
        provenance,
    }
}

fn fixture_claims(claims: &FixtureClaims) -> PumpMutationClaimsV1 {
    PumpMutationClaimsV1 {
        curve: claims.curve.as_deref().map(fixture_pubkey),
        mint: claims.mint.as_deref().map(fixture_pubkey),
        route_variant: claims
            .route_variant
            .as_deref()
            .and_then(fixture_route_variant),
        side: claims.side.as_deref().map(|side| match side {
            "buy" => PumpTradeSideV1::Buy,
            "sell" => PumpTradeSideV1::Sell,
            other => panic!("unknown fixture trade side {other}"),
        }),
        success: claims.success,
        error_code: claims.error_code.clone(),
        token_amount_units: claims.token_amount_units,
        instruction_limit: claims.instruction_limit.as_ref().map(|limit| {
            match limit.kind.as_str() {
                "max_sol_cost_lamports" => {
                    PumpInstructionLimitV1::MaxWalletDebitLamports(limit.amount)
                }
                "min_sol_output_lamports" => {
                    PumpInstructionLimitV1::MinWalletCreditLamports(limit.amount)
                }
                "exact_quote_input_lamports" => {
                    PumpInstructionLimitV1::ExactQuoteInputLamports(limit.amount)
                }
                "min_token_output_units" => {
                    PumpInstructionLimitV1::MinTokenOutputUnits(limit.amount)
                }
                other => panic!("unknown fixture instruction limit {other}"),
            }
        }),
        reported_curve_quote_lamports: claims.reported_curve_quote_lamports,
        reported_wallet_delta_lamports: claims.reported_wallet_delta_lamports,
        reported_fee_breakdown: claims.reported_fee_breakdown.as_ref().map(|fees| {
            fees.iter()
                .map(|fee| ProgramFeeCharge {
                    component_id: fee.recipient.clone(),
                    amount: fee.lamports,
                })
                .collect()
        }),
        reported_post_state_hash_blake3: claims
            .reported_post_state_hash_blake3
            .as_deref()
            .map(parse_digest),
    }
}

fn fixture_route_variant(route: &str) -> Option<PumpRouteVariant> {
    match route {
        "pump_fun_initialize" => None,
        "pump_fun_buy" => Some(PumpRouteVariant::LegacyBuy),
        "pump_fun_sell" => Some(PumpRouteVariant::LegacySell),
        other => panic!("unknown fixture route variant {other}"),
    }
}

fn fixture_pubkey(label: &str) -> Pubkey {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"ghost.pr1d.corpus.pubkey.v1");
    hasher.update(label.as_bytes());
    Pubkey::new_from_array(*hasher.finalize().as_bytes())
}

fn fixture_signature(label: &str) -> Signature {
    let mut bytes = [0_u8; 64];
    for domain in 0..2_u8 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"ghost.pr1d.corpus.signature.v1");
        hasher.update(&[domain]);
        hasher.update(label.as_bytes());
        let start = usize::from(domain) * 32;
        bytes[start..start + 32].copy_from_slice(hasher.finalize().as_bytes());
    }
    Signature::from(bytes)
}

fn parse_digest(value: &str) -> [u8; 32] {
    assert_lower_hex_digest(value);
    let mut digest = [0_u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .expect("validated lower-case digest");
    }
    digest
}

fn assert_expected_observation_result(
    scenario: &CorpusScenario,
    fixture: &CorpusObservation,
    expected: &ExpectedDecision,
    observation_decision: &PumpObservationLedgerDecisionV1,
    derived_decisions: &[PumpObservationLedgerDecisionV1],
    pending_witness_count: usize,
) {
    assert!(
        expected_classification_matches(
            &expected.classification,
            observation_decision.classification
        ),
        "production classification drifted in {} / {}: expected {}, got {:?}",
        scenario.scenario_id,
        fixture.observation_id,
        expected.classification,
        observation_decision.classification
    );
    assert_eq!(
        observation_decision.did_canonical_apply(),
        expected.canonical_apply,
        "production canonical authority drifted in {} / {}",
        scenario.scenario_id,
        fixture.observation_id
    );

    let decisions = std::iter::once(observation_decision)
        .chain(derived_decisions.iter())
        .collect::<Vec<_>>();
    assert!(
        expected_correlation_observed(
            &expected.correlation_outcome,
            fixture,
            &decisions,
            pending_witness_count,
        ),
        "production correlation drifted in {} / {}: expected {}",
        scenario.scenario_id,
        fixture.observation_id,
        expected.correlation_outcome
    );
    assert!(
        expected_agreement_observed(
            &expected.provider_agreement,
            &decisions,
            pending_witness_count,
        ),
        "production provider agreement drifted in {} / {}: expected {}",
        scenario.scenario_id,
        fixture.observation_id,
        expected.provider_agreement
    );

    let actual_conflicts: Vec<_> = decisions
        .iter()
        .flat_map(|decision| decision.conflict_fields.iter().copied())
        .collect();
    if expected.classification == "primary_raw_coverage_incomplete" {
        assert!(
            actual_conflicts.is_empty(),
            "coverage gaps are not material source conflicts"
        );
        for field in &expected.conflict_fields {
            match field.as_str() {
                "locator" => assert!(fixture.locator.is_none()),
                "canonical_order" => assert!(fixture.canonical_order.is_none()),
                "provenance" => assert!(fixture.provenance.is_none()),
                other => panic!("unknown frozen coverage field {other}"),
            }
        }
        return;
    }
    let expected_conflicts: Vec<_> = expected
        .conflict_fields
        .iter()
        .map(|field| fixture_conflict_field(field))
        .collect();
    assert_eq!(
        actual_conflicts, expected_conflicts,
        "production material conflict fields drifted in {} / {}",
        scenario.scenario_id, fixture.observation_id
    );
}

fn expected_classification_matches(
    expected: &str,
    actual: PumpObservationClassificationV1,
) -> bool {
    match expected {
        "canonical_primary_applied" => {
            actual == PumpObservationClassificationV1::PrimaryCanonicalApplied
        }
        "evidence_capacity_exceeded" => {
            actual == PumpObservationClassificationV1::EvidenceCapacityExceeded
        }
        "exact_duplicate" => actual == PumpObservationClassificationV1::ExactDuplicate,
        "pending_witness_recorded" => matches!(
            actual,
            PumpObservationClassificationV1::ParsedWitnessPending
                | PumpObservationClassificationV1::SecondaryWitnessOnly
        ),
        "primary_raw_coverage_incomplete" => {
            actual == PumpObservationClassificationV1::PrimaryRawCoverageIncomplete
        }
        "secondary_raw_witness_correlated" => {
            actual == PumpObservationClassificationV1::SameMutationAgreement
        }
        "source_reconciliation_conflict" => {
            actual == PumpObservationClassificationV1::SourceReconciliationConflict
        }
        "witness_correlated" => actual == PumpObservationClassificationV1::ExactStructuralMatch,
        other => panic!("unknown expected classification {other}"),
    }
}

fn expected_correlation_observed(
    expected: &str,
    fixture: &CorpusObservation,
    decisions: &[&PumpObservationLedgerDecisionV1],
    pending_witness_count: usize,
) -> bool {
    match expected {
        "not_observed" => decisions
            .iter()
            .all(|decision| decision.correlation.is_none()),
        "pending" => pending_witness_count > 0,
        "exact_replay_duplicate" => decisions.iter().any(|decision| {
            decision.classification == PumpObservationClassificationV1::ExactDuplicate
        }),
        "exact_structural_match" => {
            decisions.iter().any(|decision| {
                decision.correlation
                    == Some(ParsedWitnessCorrelationOutcomeV1::ExactStructuralMatch)
            }) || (fixture.source_family == "raw_yellowstone"
                && fixture.provider_role == "secondary_witness"
                && decisions.iter().any(|decision| {
                    matches!(
                        decision.classification,
                        PumpObservationClassificationV1::SameMutationAgreement
                            | PumpObservationClassificationV1::SourceReconciliationConflict
                    )
                }))
        }
        "unique_signature_singleton_match" => decisions.iter().any(|decision| {
            decision.correlation
                == Some(ParsedWitnessCorrelationOutcomeV1::UniqueSignatureSingletonMatch)
        }),
        "ambiguous" => decisions.iter().any(|decision| {
            decision.correlation == Some(ParsedWitnessCorrelationOutcomeV1::Ambiguous)
        }),
        "unmatchable" => decisions.iter().any(|decision| {
            decision.correlation == Some(ParsedWitnessCorrelationOutcomeV1::Unmatchable)
        }),
        other => panic!("unknown expected correlation {other}"),
    }
}

fn expected_agreement_observed(
    expected: &str,
    decisions: &[&PumpObservationLedgerDecisionV1],
    pending_witness_count: usize,
) -> bool {
    match expected {
        "not_observed" => decisions
            .iter()
            .all(|decision| decision.provider_agreement == PumpProviderAgreementV1::NotObserved),
        "witness_only" => {
            pending_witness_count > 0
                || decisions.iter().any(|decision| {
                    decision.provider_agreement == PumpProviderAgreementV1::WitnessOnly
                })
        }
        "primary_secondary_agreement" | "no_conflict_with_unknown" => {
            decisions.iter().any(|decision| {
                decision.provider_agreement == PumpProviderAgreementV1::PrimarySecondaryAgreement
            })
        }
        "primary_secondary_conflict" => decisions.iter().any(|decision| {
            decision.provider_agreement == PumpProviderAgreementV1::PrimarySecondaryConflict
        }),
        other => panic!("unknown expected provider agreement {other}"),
    }
}

fn assert_expected_finalize(
    scenario: &CorpusScenario,
    expected: &ExpectedFinalize,
    decision: &PumpObservationLedgerDecisionV1,
) {
    assert_eq!(expected.classification, "correlation_finalized");
    assert!(expected_correlation_observed(
        &expected.correlation_outcome,
        &CorpusObservation {
            observation_id: "finalize".to_owned(),
            source_family: "parsed_nln".to_owned(),
            provider_role: "secondary_witness".to_owned(),
            mutation_family: "trade".to_owned(),
            signature: "finalize".to_owned(),
            locator: None,
            canonical_order: None,
            raw_transaction_pump_mutation_count: None,
            claims: FixtureClaims::default(),
            provenance: None,
        },
        &[decision],
        0,
    ));
    assert!(
        expected_agreement_observed(&expected.provider_agreement, &[decision], 0),
        "production finalize agreement drifted: {}",
        scenario.scenario_id
    );
    let actual_conflicts: Vec<_> = decision.conflict_fields.clone();
    let expected_conflicts: Vec<_> = expected
        .conflict_fields
        .iter()
        .map(|field| fixture_conflict_field(field))
        .collect();
    assert_eq!(actual_conflicts, expected_conflicts);
}

fn update_replay_outcomes(
    decisions: &[&PumpObservationLedgerDecisionV1],
    last_correlation: &mut &'static str,
    integrity: &mut CandidateIntegrityOutcomeV1,
) {
    for decision in decisions {
        match decision.classification {
            PumpObservationClassificationV1::SourceReconciliationConflict => {
                *integrity = CandidateIntegrityOutcomeV1::SourceReconciliationConflict;
            }
            PumpObservationClassificationV1::PrimaryRawCoverageIncomplete => {
                *integrity = CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete;
            }
            _ => {}
        }
        if let Some(correlation) = decision.correlation {
            *last_correlation = match correlation {
                ParsedWitnessCorrelationOutcomeV1::ExactStructuralMatch => "exact_structural_match",
                ParsedWitnessCorrelationOutcomeV1::UniqueSignatureSingletonMatch => {
                    "unique_signature_singleton_match"
                }
                ParsedWitnessCorrelationOutcomeV1::Unmatchable => "unmatchable",
                ParsedWitnessCorrelationOutcomeV1::Ambiguous => "ambiguous",
            };
        } else if decision.classification == PumpObservationClassificationV1::ExactDuplicate {
            *last_correlation = "exact_replay_duplicate";
        } else if matches!(
            decision.classification,
            PumpObservationClassificationV1::SameMutationAgreement
                | PumpObservationClassificationV1::SourceReconciliationConflict
        ) && decision.provider_agreement != PumpProviderAgreementV1::NotObserved
        {
            *last_correlation = "exact_structural_match";
        }
        if let Some(signal) = &decision.candidate_integrity_signal {
            *integrity = signal.outcome;
        }
    }
}

fn candidate_integrity_label(outcome: CandidateIntegrityOutcomeV1) -> &'static str {
    match outcome {
        CandidateIntegrityOutcomeV1::Ready => "ready",
        CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete => {
            "primary_raw_coverage_incomplete"
        }
        CandidateIntegrityOutcomeV1::AccountProviderConflict => "account_provider_conflict",
        CandidateIntegrityOutcomeV1::SourceReconciliationConflict => {
            "source_reconciliation_conflict"
        }
        CandidateIntegrityOutcomeV1::AnchorMissing => "anchor_missing",
        CandidateIntegrityOutcomeV1::EconomicsNonEvaluable => "economics_non_evaluable",
    }
}

fn fixture_conflict_field(field: &str) -> PumpMutationConflictFieldV1 {
    match field {
        "curve" => PumpMutationConflictFieldV1::Curve,
        "mint" => PumpMutationConflictFieldV1::Mint,
        "route_variant" => PumpMutationConflictFieldV1::RouteVariant,
        "side" => PumpMutationConflictFieldV1::Side,
        "success" => PumpMutationConflictFieldV1::Success,
        "error_code" => PumpMutationConflictFieldV1::ErrorCode,
        "token_amount_units" => PumpMutationConflictFieldV1::TokenAmountUnits,
        "instruction_limit" => PumpMutationConflictFieldV1::InstructionLimit,
        "reported_curve_quote_lamports" => PumpMutationConflictFieldV1::ReportedCurveQuoteLamports,
        "reported_wallet_delta_lamports" => {
            PumpMutationConflictFieldV1::ReportedWalletDeltaLamports
        }
        "reported_fee_breakdown" => PumpMutationConflictFieldV1::ReportedFeeBreakdown,
        "reported_post_state_hash_blake3" => {
            PumpMutationConflictFieldV1::ReportedPostStateHashBlake3
        }
        other => panic!("unknown expected material conflict field {other}"),
    }
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
