//! PR1E executable qualification runner.
//!
//! This module is test-only, but every integrity decision is produced by the
//! production `PumpObservationLedgerV1`, `CandidateIntegrityRegistry` and Seer
//! Event Bus adapters. It does not provide a fake ledger, fake registry or a
//! second runtime authority.

use crate::{
    candidate_integrity::CandidateIntegrityRegistry,
    components::seer::{
        ingest_pump_observation, process_trade_event_for_session_gate, SessionPoolTradeBridge,
        SessionTradeDecision,
    },
    events::{create_event_bus, GhostEvent},
};
use ghost_core::{
    CandidateIntegrityOutcomeV1, CanonicalPumpOrderKeyV1, ObservationProvenanceV1,
    ObservationSourceFamilyV1, ObservedPumpMutationV1, PumpCandidateIdentityV1,
    PumpMutationClaimsV1, PumpMutationFamilyV1, PumpObservationLedgerV1, PumpTradeSideV1,
    RawProviderRoleV1, RawPumpMutationLocatorV1,
};
use seer::types::{
    InstructionProvenance, RawBytesMissingReason, ToolchainFingerprintInput, TradeEvent,
};
use serde::Deserialize;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const MANIFEST_BYTES: &[u8] = include_bytes!("../tests/fixtures/pr1e/pr1e_corpus_manifest_v1.json");
const MANIFEST_BLAKE3: &str = "cd28c798082999cf2377842199ffabb6601f7115417da244a3cf864e5ef27208";
const CROSS_LAYER_BYTES: &[u8] =
    include_bytes!("../tests/fixtures/pr1e/pr1e_cross_layer_scenarios_v1.jsonl");
const CROSS_LAYER_BLAKE3: &str = "30fbf78344afd77958fe573af5c2414139023db4c770b03c0f710026b7cdd38c";
const PR1C_V2_BYTES: &[u8] = include_bytes!(
    "../../ghost-core/tests/fixtures/account_observation_arbiter_v1/account_observation_differential_corpus_v2.jsonl"
);
const PR1D_V1_BYTES: &[u8] = include_bytes!(
    "../../ghost-core/tests/fixtures/pump_observation_ledger_v1/pump_observation_differential_corpus_v1.jsonl"
);
const PR1D_V2_BYTES: &[u8] = include_bytes!(
    "../../ghost-core/tests/fixtures/pump_observation_ledger_v2/pump_observation_differential_corpus_v2.jsonl"
);

#[derive(Debug, Deserialize)]
struct CorpusManifestV1 {
    schema: String,
    base_sha: String,
    corpora: Vec<CorpusRefV1>,
}

#[derive(Debug, Deserialize)]
struct CorpusRefV1 {
    id: String,
    path: String,
    blake3_contract: String,
    contract_kind: String,
}

#[derive(Debug, Deserialize)]
struct CrossLayerScenarioV1 {
    schema: String,
    scenario_id: String,
    phase: String,
    expected_canonical_emissions: u64,
    expected_canonical_applies: u64,
    expected_false_ready: u64,
    difference_class: Option<String>,
}

fn primary_trade_observation(
    signature: Signature,
    pool: Pubkey,
    mint: Pubkey,
    ordinal: u32,
    inventory: Option<u32>,
) -> ObservedPumpMutationV1 {
    let locator = RawPumpMutationLocatorV1 {
        program_id: Pubkey::new_from_array([4; 32]),
        signature,
        outer_instruction_index: ordinal as u16,
        inner_instruction_path: vec![ordinal as u16],
        semantic_event_ordinal: ordinal,
    };
    ObservedPumpMutationV1 {
        mutation_family: PumpMutationFamilyV1::Trade,
        signature,
        locator_hint: Some(locator.clone()),
        canonical_order: Some(CanonicalPumpOrderKeyV1 {
            slot: 10,
            tx_index: 0,
            outer_instruction_index: locator.outer_instruction_index,
            inner_instruction_path: locator.inner_instruction_path.clone(),
            semantic_event_ordinal: ordinal,
        }),
        raw_transaction_mutation_count: inventory,
        claims: PumpMutationClaimsV1 {
            curve: Some(pool),
            mint: Some(mint),
            side: Some(PumpTradeSideV1::Buy),
            success: Some(true),
            token_amount_units: Some(1_000_000 + u64::from(ordinal)),
            ..PumpMutationClaimsV1::default()
        },
        raw_provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
        provenance: ObservationProvenanceV1 {
            source_family: ObservationSourceFamilyV1::RawYellowstone,
            source_id: "yellowstone".to_string(),
            provider_id: "primary".to_string(),
            schema_id: "prost_subscribe_update_transaction_v1".to_string(),
            payload_hash_blake3: [ordinal.saturating_add(1) as u8; 32],
            received_at_monotonic_ns: u64::from(ordinal) + 1,
        },
    }
}

fn trade_carrier(signature: Signature, pool: Pubkey, mint: Pubkey, ordinal: u32) -> TradeEvent {
    TradeEvent {
        semantic: Default::default(),
        provider_id: Some("primary".to_string()),
        provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
        slot: Some(10),
        signature,
        event_ordinal: Some(ordinal),
        tx_index: Some(0),
        provenance: Some(InstructionProvenance {
            outer_instruction_index: Some(ordinal),
            inner_group_index: Some(ordinal),
            outer_program_id: Some(Pubkey::new_from_array([4; 32]).to_string()),
            invoked_program_id: Pubkey::new_from_array([4; 32]).to_string(),
            stack_height: Some(1),
            inner_instruction_path: Some(vec![ordinal as u16]),
            from_cpi: false,
        }),
        timestamp_ms: 1_000,
        arrival_ts_ms: 1_001,
        event_time: Default::default(),
        pool_amm_id: pool,
        mint,
        signer: Pubkey::new_from_array([5; 32]),
        is_buy: true,
        is_dev_buy: false,
        amount: 1_000_000 + u64::from(ordinal),
        max_sol_cost: 10_000_000,
        min_sol_output: 0,
        success: true,
        error_code: None,
        compute_units_consumed: None,
        owner_token_deltas: Vec::new(),
        mpcf_payload: vec![1, 2, 3],
        mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
        v_tokens_in_bonding_curve: Some(1_000_000.0),
        v_sol_in_bonding_curve: Some(10.0),
        virtual_sol_reserves: Some(10_000_000),
        virtual_token_reserves: Some(1_000_000),
        real_sol_reserves: Some(10_000_000),
        real_token_reserves: Some(1_000_000),
        complete: Some(false),
        market_cap_sol: None,
        global_config: None,
        fee_recipient: None,
        token_program: None,
        buy_variant: Some("legacy_buy".to_string()),
        associated_bonding_curve: Some(pool),
        creator_vault: None,
        bonding_curve_v2: None,
        bonding_curve_v2_provenance: None,
        buy_remaining_accounts: Vec::new(),
        is_mayhem_mode: None,
        cu_price_micro_lamports: None,
        compute_unit_limit: None,
        inner_ix_count: None,
        cpi_depth: None,
        ata_create_count: None,
        signer_pre_balance_lamports: None,
        signer_post_balance_lamports: None,
        jito_tip_detected: None,
        toolchain_fingerprint: ToolchainFingerprintInput::default(),
        curve_data_known: true,
        curve_finality: Default::default(),
        is_pumpswap: false,
    }
}

#[test]
fn pr1e_manifest_is_frozen_and_references_all_existing_pr1_corpora() {
    assert_eq!(
        blake3::hash(MANIFEST_BYTES).to_hex().as_str(),
        MANIFEST_BLAKE3
    );
    assert_eq!(
        blake3::hash(CROSS_LAYER_BYTES).to_hex().as_str(),
        CROSS_LAYER_BLAKE3
    );
    assert_eq!(
        blake3::hash(PR1C_V2_BYTES).to_hex().as_str(),
        "63839d047310638fe0d8643ee6c71148ac292f4390fc9098a2e573ce0ac1e051"
    );
    assert_eq!(
        blake3::hash(PR1D_V1_BYTES).to_hex().as_str(),
        "833de2bd384c964712f2e7127f9bc1db57745644633c1c66facef540cdf4c2a4"
    );
    assert_eq!(
        blake3::hash(PR1D_V2_BYTES).to_hex().as_str(),
        "c81d7b4f0cc3792c2bb2c4e71bfd0634fcfdd69723758d741ee2405770603415"
    );

    let manifest: CorpusManifestV1 =
        serde_json::from_slice(MANIFEST_BYTES).expect("valid PR1E manifest");
    assert_eq!(manifest.schema, "pr1e_corpus_manifest_v1");
    assert_eq!(
        manifest.base_sha,
        "103212b16bfc059db367e1ceb3c7d00fd307d6c5"
    );
    assert_eq!(manifest.corpora.len(), 5);
    assert!(manifest.corpora.iter().all(|entry| {
        !entry.id.is_empty()
            && !entry.path.is_empty()
            && entry.blake3_contract.len() == 64
            && matches!(
                entry.contract_kind.as_str(),
                "file_digest" | "embedded_snapshot_digest"
            )
    }));

    let scenarios = std::str::from_utf8(CROSS_LAYER_BYTES)
        .expect("UTF-8 JSONL")
        .lines()
        .map(|line| serde_json::from_str::<CrossLayerScenarioV1>(line).expect("valid scenario"))
        .collect::<Vec<_>>();
    assert_eq!(scenarios.len(), 23);
    assert!(scenarios.iter().all(|scenario| {
        scenario.schema == "pr1e_cross_layer_scenario_v1"
            && !scenario.phase.is_empty()
            && scenario.expected_false_ready == 0
            && (scenario.phase == "account_state"
                || scenario.expected_canonical_applies <= scenario.expected_canonical_emissions)
            && scenario
                .difference_class
                .as_ref()
                .is_none_or(|class| !class.is_empty())
    }));
    let actual = scenarios
        .iter()
        .map(|scenario| scenario.scenario_id.as_str())
        .collect::<BTreeSet<_>>();
    let required = BTreeSet::from([
        "primary_create_session_apply_ready",
        "create_and_initial_buy_one_signature",
        "two_trade_mutations_one_signature",
        "duplicate_raw_after_first_apply",
        "raw_nln_agreement",
        "nln_first_raw_second",
        "nln_only",
        "raw_nln_conflict_before_mfs",
        "conflict_during_mfs",
        "conflict_during_evaluation",
        "conflict_buy_before_submit",
        "conflict_race_with_submit",
        "conflict_after_submit",
        "conflict_after_confirmation",
        "missing_transport_observation",
        "wrapper_observation_provider_mismatch",
        "buffered_canonical_trade_replay",
        "buffered_trade_expiry",
        "account_update_duplicate",
        "account_same_version_different_hash",
        "continuity_only_restored_position",
        "writer_stall",
        "queue_saturation",
    ]);
    assert_eq!(actual, required);
}

#[tokio::test]
async fn pr1e_runner_uses_production_ledger_registry_and_event_adapter() {
    let ledger = Arc::new(Mutex::new(PumpObservationLedgerV1::default()));
    let registry = Arc::new(CandidateIntegrityRegistry::default());
    let pool = Pubkey::new_from_array([1; 32]);
    let mint = Pubkey::new_from_array([2; 32]);
    let signature = Signature::from([3; 64]);
    let observation = primary_trade_observation(signature, pool, mint, 0, Some(1));

    let receipt =
        ingest_pump_observation(&ledger, &registry, Some(observation.clone()), 1, true, None)
            .expect("unique primary produces one canonical receipt");

    let (event_tx, mut event_rx) = create_event_bus();
    let mut bridge =
        SessionPoolTradeBridge::new(Duration::from_secs(1), 4, 16, Duration::from_secs(60), 32);
    let _ = bridge.register_detected_pool(pool, Instant::now());
    let trade = trade_carrier(signature, pool, mint, 0);
    let ingress = process_trade_event_for_session_gate(
        &event_tx,
        &mut bridge,
        &trade,
        None,
        Instant::now(),
        Some(&receipt),
        Some(&registry),
    );
    assert_eq!(ingress.decision, SessionTradeDecision::ForwardNow);
    assert!(matches!(
        event_rx
            .recv()
            .await
            .expect("production Event Bus emission"),
        GhostEvent::PoolTransaction(_)
    ));

    let candidate = PumpCandidateIdentityV1 {
        pool_amm_id: pool,
        mint,
    };
    assert!(registry.evaluation_guard(candidate).is_err());
    let ready_signals = {
        let mut replay = PumpObservationLedgerV1::default();
        let result = replay.observe(observation.clone(), 1);
        std::iter::once(&result.observation_decision)
            .chain(result.derived_decisions.iter())
            .filter_map(|decision| decision.candidate_integrity_signal.clone())
            .filter(|signal| signal.outcome == CandidateIntegrityOutcomeV1::Ready)
            .collect::<Vec<_>>()
    };
    registry
        .seal_complete_transaction_inventory(signature, &ready_signals)
        .expect("complete inventory seals");
    registry
        .mark_canonical_apply_succeeded(&receipt)
        .expect("real apply acknowledgement")
        .first()
        .expect("candidate released after apply");
    assert!(registry.evaluation_guard(candidate).is_ok());

    assert!(
        ingest_pump_observation(&ledger, &registry, Some(observation.clone()), 2, true, None,)
            .is_none()
    );

    let mut secondary = observation.clone();
    secondary.raw_provider_role = Some(RawProviderRoleV1::SecondaryWitness);
    secondary.provenance.provider_id = "secondary".to_string();
    secondary.provenance.payload_hash_blake3 = [9; 32];
    assert!(ingest_pump_observation(&ledger, &registry, Some(secondary), 3, true, None,).is_none());

    let mut nln = observation;
    nln.locator_hint = None;
    nln.canonical_order = None;
    nln.raw_provider_role = None;
    nln.provenance.source_family = ObservationSourceFamilyV1::ParsedNln;
    nln.provenance.provider_id = "nln".to_string();
    nln.provenance.payload_hash_blake3 = [10; 32];
    assert!(ingest_pump_observation(&ledger, &registry, Some(nln), 4, true, None,).is_none());

    let snapshot = ledger.lock().expect("ledger").snapshot();
    assert_eq!(snapshot.canonical_mutation_count, 1);
}
