//! Frozen differential corpus gate for PR1C AccountObservationArbiter.
//!
//! This test intentionally validates the corpus before the reducer behavior is
//! changed.  The replay assertion is added together with the arbiter; the
//! digest and mandatory scenario inventory make the fixture itself immutable
//! enough to serve as the pre-implementation differential contract.

use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::account_state_core::types::{AccountStateUpdate, UpdateSource};
use ghost_core::account_state_core::{
    AccountObservationClassificationV1, AccountObservationOutcomeV1, AccountProviderAgreementV1,
};
use ghost_core::{CurveFinality, RawProviderRoleV1};
use serde::Deserialize;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use std::collections::BTreeSet;

const CORPUS_V1_BYTES: &[u8] = include_bytes!(
    "fixtures/account_observation_arbiter_v1/account_observation_differential_corpus_v1.jsonl"
);
const CORPUS_V2_BYTES: &[u8] = include_bytes!(
    "fixtures/account_observation_arbiter_v1/account_observation_differential_corpus_v2.jsonl"
);

// Updated only by an explicit v2 corpus migration.  This was recorded before
// the PR1C arbiter behavior was introduced.
const CORPUS_BLAKE3_HEX: &str = "12472c3e8f43f28185b520f3c93a3c1e04d376a46347f42c5935c6b53665d706";
const CORPUS_V2_BLAKE3_HEX: &str =
    "63839d047310638fe0d8643ee6c71148ac292f4390fc9098a2e573ce0ac1e051";

#[derive(Debug, Deserialize)]
struct CorpusScenario {
    schema_version: u32,
    scenario_id: String,
    account_key: String,
    base_mint_key: String,
    observations: Vec<CorpusObservation>,
    expected: Vec<ExpectedDecision>,
}

#[derive(Debug, Deserialize)]
struct CorpusObservation {
    provider_id: String,
    provider_role: RawProviderRoleV1,
    slot: u64,
    write_version: Option<u64>,
    data_hash_blake3: String,
    signature_tag: String,
    receive_seq: u64,
}

#[derive(Debug, Deserialize)]
struct ExpectedDecision {
    classification: AccountObservationClassificationV1,
    outcome: AccountObservationOutcomeV1,
    canonical_apply: bool,
    provider_agreement: AccountProviderAgreementV1,
}

#[test]
fn account_observation_differential_corpus_v1_is_frozen_and_complete() {
    let actual_digest = blake3::hash(CORPUS_V1_BYTES).to_hex().to_string();
    assert_eq!(
        actual_digest, CORPUS_BLAKE3_HEX,
        "the PR1C account-observation corpus changed; create an explicit new corpus version instead of rewriting v1"
    );

    let scenarios: Vec<CorpusScenario> = std::str::from_utf8(CORPUS_V1_BYTES)
        .expect("corpus must be UTF-8 JSONL")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("every corpus line must be valid JSON"))
        .collect();

    let scenario_ids: BTreeSet<_> = scenarios
        .iter()
        .map(|scenario| scenario.scenario_id.as_str())
        .collect();
    let required = BTreeSet::from([
        "same_provider_exact_duplicate",
        "two_providers_identical_observation",
        "same_version_same_hash_different_signature",
        "same_version_different_hash_conflict",
        "older_write_version_after_newer",
        "write_version_none_same_version_duplicate",
        "none_vs_some_zero_same_slot",
        "reconnect_replay_duplicate",
        "secondary_first_then_primary_agrees",
        "secondary_first_then_primary_conflicts",
        "many_updates_one_account_series",
    ]);

    assert_eq!(
        scenario_ids, required,
        "hard-gate scenario inventory drifted"
    );
    assert!(scenarios.iter().all(|scenario| {
        scenario.schema_version == 1
            && !scenario.account_key.is_empty()
            && !scenario.base_mint_key.is_empty()
            && !scenario.observations.is_empty()
            && scenario.observations.len() == scenario.expected.len()
    }));
}

#[test]
fn account_observation_differential_corpus_v2_is_frozen_and_replays_through_canonical_arbiter() {
    let actual_digest = blake3::hash(CORPUS_V2_BYTES).to_hex().to_string();
    assert_eq!(
        actual_digest, CORPUS_V2_BLAKE3_HEX,
        "the corrected PR1C corpus changed; create an explicit next corpus version instead of rewriting v2"
    );
    let scenarios = parse_corpus(CORPUS_V2_BYTES);

    assert!(scenarios
        .iter()
        .all(|scenario| scenario.schema_version == 2));
    let scenario_ids: BTreeSet<_> = scenarios
        .iter()
        .map(|scenario| scenario.scenario_id.as_str())
        .collect();
    let required = BTreeSet::from([
        "same_provider_exact_duplicate",
        "two_providers_identical_observation",
        "same_version_same_hash_different_signature",
        "same_version_different_hash_conflict",
        "older_write_version_after_newer",
        "write_version_none_same_version_duplicate",
        "none_vs_some_zero_same_slot",
        "reconnect_replay_duplicate",
        "secondary_first_then_primary_agrees",
        "secondary_first_then_primary_conflicts",
        "many_updates_one_account_series",
    ]);
    assert_eq!(
        scenario_ids, required,
        "v2 must preserve the hard-gate inventory"
    );

    for scenario in scenarios {
        let reducer = AccountStateReducer::new();
        let account = pubkey_for(&scenario.account_key);
        let base_mint = pubkey_for(&scenario.base_mint_key);
        let pool = pubkey_for(&format!("pool:{}", scenario.base_mint_key));
        let owner = pubkey_for(&format!("owner:{}", scenario.account_key));
        let mut expected_applies = 0_u64;

        for (index, (observation, expected)) in scenario
            .observations
            .iter()
            .zip(scenario.expected.iter())
            .enumerate()
        {
            let before_state = reducer.get_canonical_state(&base_mint);
            let before_velocity = reducer.get_reserve_velocity_snapshot(&base_mint);
            let update = AccountStateUpdate {
                pool_amm_id: pool,
                base_mint,
                bonding_curve: account,
                sol_reserves: 1_000_000_000_u64.saturating_add(index as u64),
                token_reserves: 500_000_000_000_u64.saturating_add(index as u64),
                is_complete: 0,
                slot: observation.slot,
                write_version: observation.write_version,
                source_account_pubkey: Some(account),
                source_account_owner_or_program: Some(owner),
                account_data_len: Some(56),
                account_data_hash: Some(observation.data_hash_blake3.clone()),
                receive_ts_ms: 10_000_u64.saturating_add(index as u64),
                receive_seq: observation.receive_seq,
                curve_finality: CurveFinality::Finalized,
                source: UpdateSource::GeyserAccountUpdate,
                provider_id: Some(observation.provider_id.clone()),
                provider_role: Some(observation.provider_role),
                txn_signature: Some(signature_for(&observation.signature_tag)),
            };

            let result = reducer.apply_account_observation(update);
            assert_eq!(
                result.decision.classification, expected.classification,
                "scenario={} observation={index}",
                scenario.scenario_id
            );
            assert_eq!(
                result.decision.outcome, expected.outcome,
                "scenario={} observation={index}",
                scenario.scenario_id
            );
            assert_eq!(
                result.decision.provider_agreement, expected.provider_agreement,
                "scenario={} observation={index}",
                scenario.scenario_id
            );
            assert_eq!(
                result.did_apply(),
                expected.canonical_apply,
                "scenario={} observation={index}",
                scenario.scenario_id
            );

            if expected.canonical_apply {
                expected_applies = expected_applies.saturating_add(1);
            } else {
                assert_eq!(
                    reducer.get_canonical_state(&base_mint),
                    before_state,
                    "non-canonical observation changed reserves/state: scenario={} observation={index}",
                    scenario.scenario_id
                );
                assert_eq!(
                    reducer.get_reserve_velocity_snapshot(&base_mint),
                    before_velocity,
                    "non-canonical observation changed reserve velocity evidence: scenario={} observation={index}",
                    scenario.scenario_id
                );
            }
        }

        let snapshot = reducer
            .account_observation_arbiter_snapshot(&base_mint)
            .expect("all corpus observations must create an arbiter");
        assert_eq!(
            snapshot.counters.canonical_mutation_count, expected_applies,
            "scenario={} canonical mutation count drifted",
            scenario.scenario_id
        );
        let canonical_state = reducer.get_canonical_state(&base_mint);
        assert_eq!(canonical_state.is_some(), expected_applies > 0);
        if let Some(canonical_state) = canonical_state {
            assert_eq!(canonical_state.update_count, expected_applies);
            assert_eq!(canonical_state.data_change_count, expected_applies);
            assert_eq!(canonical_state.observation_count, expected_applies);
        }
    }
}

fn parse_corpus(bytes: &[u8]) -> Vec<CorpusScenario> {
    std::str::from_utf8(bytes)
        .expect("corpus must be UTF-8 JSONL")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("every corpus line must be valid JSON"))
        .collect()
}

fn pubkey_for(label: &str) -> Pubkey {
    Pubkey::new_from_array(*blake3::hash(label.as_bytes()).as_bytes())
}

fn signature_for(label: &str) -> Signature {
    let digest = blake3::hash(label.as_bytes());
    let mut bytes = [0_u8; 64];
    bytes[..32].copy_from_slice(digest.as_bytes());
    bytes[32..].copy_from_slice(digest.as_bytes());
    Signature::from(bytes)
}
