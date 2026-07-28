//! PR1E executable qualification runner.
//!
//! This module is test-only, but every integrity decision is produced by the
//! production `PumpObservationLedgerV1`, `CandidateIntegrityRegistry` and Seer
//! Event Bus adapters. It does not provide a fake ledger, fake registry or a
//! second runtime authority.

use crate::{
    candidate_integrity::{CandidateIntegrityRegistry, CandidateTerminalTransitionV1},
    components::seer::{
        authorize_pool_runtime_disposition, handle_local_coverage_gap_notice,
        ingest_pump_observation, process_trade_event_for_session_gate,
        replay_buffered_canonical_trades, CanonicalRuntimeAdmissionV1,
        CanonicalRuntimeNoApplyReasonV1, SessionPoolTradeBridge, SessionTradeDecision,
    },
    events::{create_event_bus, CanonicalRuntimePermitV1, GhostEvent},
    session::{observation::CanonicalMutationApplyOutcomeV1, PoolObservationSession},
    tx_intelligence::TxIntelligenceConfig,
};
use ghost_brain::{config::GatekeeperV2Config, fast_pipeline::EnhancedCandidate};
use ghost_core::{
    account_state_core::{
        reducer::AccountStateReducer,
        types::{AccountStateUpdate, BootstrapHints, UpdateSource},
    },
    CandidateIntegrityOutcomeV1, CandidateIntegritySignalV1, CanonicalPumpOrderKeyV1,
    CurveFinality, ObservationProvenanceV1, ObservationSourceFamilyV1, ObservedPumpMutationV1,
    PumpCandidateIdentityV1, PumpMutationClaimsV1, PumpMutationConflictFieldV1,
    PumpMutationFamilyV1, PumpObservationLedgerV1, PumpTradeSideV1, RawProviderRoleV1,
    RawPumpMutationLocatorV1,
};
use seer::{
    early_fingerprint::EarlyFingerprintConfig,
    ipc::{LocalCoverageGapNoticeV1, PoolDetectionRuntimeDispositionV1},
    types::{InstructionProvenance, RawBytesMissingReason, ToolchainFingerprintInput, TradeEvent},
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

/// Machine-readable result of one frozen cross-layer scenario after it has
/// traversed the production ledger, permit boundary, Event Bus adapter and
/// real `PoolObservationSession` mutation owner.  It deliberately records
/// runtime facts rather than re-stating fixture expectations.
#[derive(Debug, Default, PartialEq, Eq)]
struct CrossLayerExecutionV1 {
    canonical_emissions: u64,
    downstream_applies: u64,
    ready_publications: u64,
    mfs_materializations: u64,
    gatekeeper_invocations: u64,
    sender_calls: u64,
    false_ready: u64,
    difference_class: Option<&'static str>,
}

fn qualification_session(pool: Pubkey, mint: Pubkey) -> PoolObservationSession {
    let gatekeeper_config = GatekeeperV2Config::default();
    let candidate = EnhancedCandidate {
        pool_amm_id: pool,
        base_mint: mint,
        quote_mint: Pubkey::new_unique(),
        bonding_curve: pool,
        slot: Some(10),
        timestamp: 1_000,
        signature: Signature::new_unique().to_string(),
        ..EnhancedCandidate::default()
    };
    PoolObservationSession::new(
        ghost_core::session::types::SessionId(1),
        pool,
        mint,
        pool,
        None,
        candidate,
        1_000,
        61_000,
        &gatekeeper_config,
        TxIntelligenceConfig::from_gatekeeper_config(
            &gatekeeper_config,
            EarlyFingerprintConfig::default(),
        ),
    )
}

fn primary_permit(
    ledger: &Arc<Mutex<PumpObservationLedgerV1>>,
    registry: &Arc<CandidateIntegrityRegistry>,
    observation: ObservedPumpMutationV1,
    now_monotonic_ns: u64,
) -> CanonicalRuntimePermitV1 {
    match ingest_pump_observation(
        ledger,
        registry,
        Some(observation),
        now_monotonic_ns,
        true,
        None,
    ) {
        CanonicalRuntimeAdmissionV1::Apply(permit) => permit,
        other => panic!("frozen primary scenario must obtain one canonical permit: {other:?}"),
    }
}

/// The actual downstream state owner is `PoolObservationSession`; a receipt
/// is completed only after its typed apply result says `AppliedNewMutation`.
/// This is intentionally not a direct registry acknowledgement.
fn forward_permitted_trade_through_session(
    registry: &Arc<CandidateIntegrityRegistry>,
    bridge: &mut SessionPoolTradeBridge,
    session: &mut PoolObservationSession,
    trade: &TradeEvent,
    permit: CanonicalRuntimePermitV1,
    now: Instant,
) -> CrossLayerExecutionV1 {
    let (event_tx, mut event_rx) = create_event_bus();
    let _ = bridge.register_detected_pool(trade.pool_amm_id, now);
    let ingress =
        process_trade_event_for_session_gate(&event_tx, bridge, trade, None, now, permit, registry);
    assert_eq!(ingress.decision, SessionTradeDecision::ForwardNow);

    let (tx, receipt) = match event_rx.try_recv().expect("canonical Event Bus delivery") {
        GhostEvent::PoolTransaction(tx, Some(permit)) => (tx, permit.apply_receipt),
        other => {
            panic!("production adapter must emit one permitted PoolTransaction, got {other:?}")
        }
    };
    let result = session.ingest_transaction_with_apply_result(tx);
    let mut execution = CrossLayerExecutionV1 {
        canonical_emissions: 1,
        ..CrossLayerExecutionV1::default()
    };
    let completion = complete_receipt_after_session_apply(registry, &receipt, result.apply);
    execution.downstream_applies = completion.downstream_applies;
    execution.ready_publications = completion.ready_publications;
    execution
}

/// The qualification runner has exactly one receipt-completion boundary.  It
/// is deliberately reached only after the real session owner returns its
/// typed apply outcome; all non-apply outcomes fail the original permit.
fn complete_receipt_after_session_apply(
    registry: &CandidateIntegrityRegistry,
    receipt: &crate::candidate_integrity::CanonicalMutationApplyReceiptV1,
    apply: CanonicalMutationApplyOutcomeV1,
) -> CrossLayerExecutionV1 {
    match apply {
        CanonicalMutationApplyOutcomeV1::AppliedNewMutation => CrossLayerExecutionV1 {
            downstream_applies: 1,
            ready_publications: registry
                .mark_canonical_apply_succeeded(receipt)
                .expect("typed downstream apply must complete the matching receipt")
                .len() as u64,
            ..CrossLayerExecutionV1::default()
        },
        CanonicalMutationApplyOutcomeV1::Duplicate
        | CanonicalMutationApplyOutcomeV1::Ignored
        | CanonicalMutationApplyOutcomeV1::Terminal
        | CanonicalMutationApplyOutcomeV1::Failed => {
            registry
                .fail_canonical_apply(receipt)
                .expect("non-apply downstream result must fail its receipt");
            CrossLayerExecutionV1::default()
        }
    }
}

fn lifecycle_conflict_signal(
    candidate: PumpCandidateIdentityV1,
    signature: Signature,
    locator: RawPumpMutationLocatorV1,
) -> CandidateIntegritySignalV1 {
    CandidateIntegritySignalV1 {
        candidate,
        outcome: CandidateIntegrityOutcomeV1::SourceReconciliationConflict,
        signature: Some(signature),
        locator: Some(locator),
        conflict_fields: vec![PumpMutationConflictFieldV1::TokenAmountUnits],
        evidence_hash_blake3: [0xCC; 32],
    }
}

fn primary_account_update(
    pool: Pubkey,
    mint: Pubkey,
    curve: Pubkey,
    slot: u64,
    write_version: u64,
    hash_byte: char,
) -> AccountStateUpdate {
    AccountStateUpdate {
        pool_amm_id: pool,
        base_mint: mint,
        bonding_curve: curve,
        sol_reserves: 30_000_000_000 + slot,
        token_reserves: 900_000_000_000_000 - slot,
        is_complete: 0,
        slot,
        write_version: Some(write_version),
        source_account_pubkey: Some(curve),
        source_account_owner_or_program: Some(Pubkey::new_from_array([9; 32])),
        account_data_len: Some(56),
        account_data_hash: Some(std::iter::repeat_n(hash_byte, 64).collect()),
        receive_ts_ms: slot,
        receive_seq: slot,
        curve_finality: CurveFinality::Speculative,
        source: UpdateSource::GeyserAccountUpdate,
        provider_id: Some("primary".to_string()),
        provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
        txn_signature: None,
    }
}

fn execute_account_state_scenario(conflict: bool) -> CrossLayerExecutionV1 {
    let reducer = AccountStateReducer::new();
    let pool = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let curve = Pubkey::new_unique();
    reducer.register_pool_from_bootstrap(pool, mint, curve, BootstrapHints::default());
    let first = primary_account_update(pool, mint, curve, 10, 1, 'a');
    assert!(reducer.apply_account_observation(first.clone()).did_apply());
    let second = if conflict {
        primary_account_update(pool, mint, curve, 10, 1, 'b')
    } else {
        first
    };
    assert!(!reducer.apply_account_observation(second).did_apply());
    CrossLayerExecutionV1 {
        downstream_applies: 1,
        difference_class: Some(if conflict {
            "ACCOUNT_PROVIDER_CONFLICT"
        } else {
            "ACCOUNT_DUPLICATE_SUPPRESSED"
        }),
        ..CrossLayerExecutionV1::default()
    }
}

fn parsed_witness_from_primary(
    primary: &ObservedPumpMutationV1,
    conflicting_amount: Option<u64>,
) -> ObservedPumpMutationV1 {
    let mut witness = primary.clone();
    witness.canonical_order = None;
    witness.raw_provider_role = None;
    witness.provenance.source_family = ObservationSourceFamilyV1::ParsedNln;
    witness.provenance.source_id = "nln".to_string();
    witness.provenance.provider_id = "nln".to_string();
    witness.provenance.schema_id = "nln_pump_trade_v1".to_string();
    witness.provenance.payload_hash_blake3 = [0xD1; 32];
    if let Some(amount) = conflicting_amount {
        witness.claims.token_amount_units = Some(amount);
    }
    witness
}

fn apply_unique_primary(
    ledger: &Arc<Mutex<PumpObservationLedgerV1>>,
    registry: &Arc<CandidateIntegrityRegistry>,
    bridge: &mut SessionPoolTradeBridge,
    session: &mut PoolObservationSession,
    signature: Signature,
    pool: Pubkey,
    mint: Pubkey,
    ordinal: u32,
    inventory: Option<u32>,
    now_monotonic_ns: u64,
) -> (CanonicalRuntimePermitV1, CrossLayerExecutionV1) {
    let permit = primary_permit(
        ledger,
        registry,
        primary_trade_observation(signature, pool, mint, ordinal, inventory),
        now_monotonic_ns,
    );
    let execution = forward_permitted_trade_through_session(
        registry,
        bridge,
        session,
        &trade_carrier(signature, pool, mint, ordinal),
        permit.clone(),
        Instant::now(),
    );
    (permit, execution)
}

fn add_execution(target: &mut CrossLayerExecutionV1, next: CrossLayerExecutionV1) {
    target.canonical_emissions = target
        .canonical_emissions
        .saturating_add(next.canonical_emissions);
    target.downstream_applies = target
        .downstream_applies
        .saturating_add(next.downstream_applies);
    target.ready_publications = target
        .ready_publications
        .saturating_add(next.ready_publications);
    target.mfs_materializations = target
        .mfs_materializations
        .saturating_add(next.mfs_materializations);
    target.gatekeeper_invocations = target
        .gatekeeper_invocations
        .saturating_add(next.gatekeeper_invocations);
    target.sender_calls = target.sender_calls.saturating_add(next.sender_calls);
    target.false_ready = target.false_ready.saturating_add(next.false_ready);
    if target.difference_class.is_none() {
        target.difference_class = next.difference_class;
    }
}

fn assert_no_runtime_permit(admission: CanonicalRuntimeAdmissionV1) {
    assert!(
        matches!(
            admission,
            CanonicalRuntimeAdmissionV1::NoApply(_) | CanonicalRuntimeAdmissionV1::Blocked(_)
        ),
        "witness, duplicate or integrity-failed input must never obtain a canonical runtime permit"
    );
}

/// Execute one frozen scenario against the production ledger, registry,
/// canonical permit boundary, Event Bus adapter and session mutation owner.
/// The fixture has no embedded fake outcome: each `scenario_id` selects a
/// concrete production action sequence and the returned counters are the
/// actual result compared with its immutable JSONL expectation below.
fn execute_cross_layer_scenario(scenario_id: &str) -> CrossLayerExecutionV1 {
    let ledger = Arc::new(Mutex::new(PumpObservationLedgerV1::default()));
    let registry = Arc::new(CandidateIntegrityRegistry::default());
    let pool = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let signature = Signature::new_unique();
    let candidate = PumpCandidateIdentityV1 {
        pool_amm_id: pool,
        mint,
    };
    let mut bridge =
        SessionPoolTradeBridge::new(Duration::from_secs(1), 4, 16, Duration::from_secs(60), 32);
    let mut session = qualification_session(pool, mint);

    match scenario_id {
        "primary_create_session_apply_ready" | "writer_stall" => {
            let (_, execution) = apply_unique_primary(
                &ledger,
                &registry,
                &mut bridge,
                &mut session,
                signature,
                pool,
                mint,
                0,
                Some(1),
                1,
            );
            assert_eq!(execution.ready_publications, 1);
            execution
        }
        "create_and_initial_buy_one_signature" | "two_trade_mutations_one_signature" => {
            let mut execution = CrossLayerExecutionV1::default();
            let (_, first) = apply_unique_primary(
                &ledger,
                &registry,
                &mut bridge,
                &mut session,
                signature,
                pool,
                mint,
                0,
                Some(2),
                1,
            );
            assert_eq!(
                first.ready_publications, 0,
                "first locator is not Ready alone"
            );
            add_execution(&mut execution, first);
            let (_, second) = apply_unique_primary(
                &ledger,
                &registry,
                &mut bridge,
                &mut session,
                signature,
                pool,
                mint,
                1,
                Some(2),
                2,
            );
            assert_eq!(
                second.ready_publications, 1,
                "final exact locator releases once"
            );
            add_execution(&mut execution, second);
            execution
        }
        "duplicate_raw_after_first_apply" => {
            let primary = primary_trade_observation(signature, pool, mint, 0, Some(1));
            let permit = primary_permit(&ledger, &registry, primary.clone(), 1);
            let execution = forward_permitted_trade_through_session(
                &registry,
                &mut bridge,
                &mut session,
                &trade_carrier(signature, pool, mint, 0),
                permit,
                Instant::now(),
            );
            assert_eq!(execution.ready_publications, 1);
            assert!(matches!(
                ingest_pump_observation(&ledger, &registry, Some(primary), 2, true, None),
                CanonicalRuntimeAdmissionV1::NoApply(
                    CanonicalRuntimeNoApplyReasonV1::ExactDuplicate
                )
            ));
            CrossLayerExecutionV1 {
                difference_class: Some("DUPLICATE_PRIMARY_SUPPRESSED"),
                ..execution
            }
        }
        "raw_nln_agreement" | "nln_first_raw_second" | "raw_nln_conflict_before_mfs" => {
            let primary = primary_trade_observation(signature, pool, mint, 0, Some(1));
            let conflict = scenario_id == "raw_nln_conflict_before_mfs";
            let witness = parsed_witness_from_primary(&primary, conflict.then_some(42));
            if scenario_id == "nln_first_raw_second" || conflict {
                assert_no_runtime_permit(ingest_pump_observation(
                    &ledger,
                    &registry,
                    Some(witness.clone()),
                    1,
                    true,
                    None,
                ));
            }
            let permit = primary_permit(&ledger, &registry, primary, 2);
            let execution = forward_permitted_trade_through_session(
                &registry,
                &mut bridge,
                &mut session,
                &trade_carrier(signature, pool, mint, 0),
                permit,
                Instant::now(),
            );
            if scenario_id == "raw_nln_agreement" {
                assert_no_runtime_permit(ingest_pump_observation(
                    &ledger,
                    &registry,
                    Some(witness),
                    3,
                    true,
                    None,
                ));
            }
            if conflict {
                assert!(registry.evaluation_guard(candidate).is_err());
                CrossLayerExecutionV1 {
                    difference_class: Some("SOURCE_RECONCILIATION_BLOCK"),
                    ..execution
                }
            } else {
                assert_eq!(execution.ready_publications, 1);
                CrossLayerExecutionV1 {
                    difference_class: Some("PARSED_WITNESS_SUPPRESSED"),
                    ..execution
                }
            }
        }
        "nln_only" => {
            let primary = primary_trade_observation(signature, pool, mint, 0, Some(1));
            assert_no_runtime_permit(ingest_pump_observation(
                &ledger,
                &registry,
                Some(parsed_witness_from_primary(&primary, None)),
                1,
                true,
                None,
            ));
            CrossLayerExecutionV1 {
                difference_class: Some("PARSED_WITNESS_SUPPRESSED"),
                ..CrossLayerExecutionV1::default()
            }
        }
        "conflict_during_mfs"
        | "conflict_during_evaluation"
        | "conflict_buy_before_submit"
        | "conflict_race_with_submit"
        | "conflict_after_submit"
        | "conflict_after_confirmation" => {
            let (permit, mut execution) = apply_unique_primary(
                &ledger,
                &registry,
                &mut bridge,
                &mut session,
                signature,
                pool,
                mint,
                0,
                Some(1),
                1,
            );
            assert_eq!(execution.ready_publications, 1);
            let guard = registry
                .evaluation_guard(candidate)
                .expect("Ready before conflict");
            guard.mark_mfs_materialized().expect("MFS phase");
            execution.mfs_materializations = 1;
            if scenario_id != "conflict_during_mfs" {
                guard.mark_evaluation_running().expect("evaluation phase");
            }

            let difference_class = match scenario_id {
                "conflict_during_mfs" | "conflict_during_evaluation" => {
                    registry
                        .record_signal(lifecycle_conflict_signal(
                            candidate,
                            signature,
                            permit.locator.clone(),
                        ))
                        .expect("typed conflict");
                    assert!(guard.check_ready().is_err());
                    "EVALUATION_TECHNICALLY_ABORTED"
                }
                "conflict_buy_before_submit" => {
                    let submit = guard
                        .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
                        .expect("BUY terminal")
                        .expect("submit guard");
                    registry
                        .record_signal(lifecycle_conflict_signal(
                            candidate,
                            signature,
                            permit.locator.clone(),
                        ))
                        .expect("typed conflict");
                    assert!(submit.try_begin_submit().is_err());
                    "EXECUTION_CANCELLED_BEFORE_SUBMIT"
                }
                "conflict_race_with_submit" | "conflict_after_submit" => {
                    let submit = guard
                        .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
                        .expect("BUY terminal")
                        .expect("submit guard");
                    assert!(submit.try_begin_submit().is_ok());
                    execution.sender_calls = 1;
                    registry
                        .record_signal(lifecycle_conflict_signal(
                            candidate,
                            signature,
                            permit.locator.clone(),
                        ))
                        .expect("typed conflict");
                    assert!(submit.requires_reconciliation());
                    "POST_SUBMIT_RECONCILIATION_REQUIRED"
                }
                "conflict_after_confirmation" => {
                    let submit = guard
                        .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
                        .expect("BUY terminal")
                        .expect("submit guard");
                    assert!(submit.try_begin_submit().is_ok());
                    submit
                        .mark_confirmed()
                        .expect("confirmation remains authoritative");
                    execution.sender_calls = 1;
                    registry
                        .record_signal(lifecycle_conflict_signal(
                            candidate,
                            signature,
                            permit.locator.clone(),
                        ))
                        .expect("late conflict audit");
                    assert!(
                        registry
                            .snapshot(candidate)
                            .expect("confirmed history")
                            .witness_quarantined
                    );
                    "CONFIRMED_POSITION_WITNESS_QUARANTINED"
                }
                _ => unreachable!("matched lifecycle scenario"),
            };
            execution.difference_class = Some(difference_class);
            execution
        }
        "missing_transport_observation" => {
            assert!(matches!(
                ingest_pump_observation(&ledger, &registry, None, 1, true, None),
                CanonicalRuntimeAdmissionV1::Blocked(
                    CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
                )
            ));
            CrossLayerExecutionV1 {
                difference_class: Some("PRIMARY_BOUNDARY_INCOMPLETE"),
                ..CrossLayerExecutionV1::default()
            }
        }
        "wrapper_observation_provider_mismatch" => {
            let primary = primary_trade_observation(signature, pool, mint, 0, Some(1));
            assert!(matches!(
                ingest_pump_observation(
                    &ledger,
                    &registry,
                    Some(primary),
                    1,
                    false,
                    Some(lifecycle_conflict_signal(
                        candidate,
                        signature,
                        RawPumpMutationLocatorV1 {
                            program_id: Pubkey::new_unique(),
                            signature,
                            outer_instruction_index: 0,
                            inner_instruction_path: Vec::new(),
                            semantic_event_ordinal: 0,
                        },
                    )),
                ),
                CanonicalRuntimeAdmissionV1::Blocked(
                    CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
                )
            ));
            CrossLayerExecutionV1 {
                difference_class: Some("PRIMARY_BOUNDARY_INCOMPLETE"),
                ..CrossLayerExecutionV1::default()
            }
        }
        "buffered_canonical_trade_replay" => {
            let permit = primary_permit(
                &ledger,
                &registry,
                primary_trade_observation(signature, pool, mint, 0, Some(1)),
                1,
            );
            let trade = trade_carrier(signature, pool, mint, 0);
            let (event_tx, mut event_rx) = create_event_bus();
            let now = Instant::now();
            assert_eq!(
                process_trade_event_for_session_gate(
                    &event_tx,
                    &mut bridge,
                    &trade,
                    None,
                    now,
                    permit,
                    &registry,
                )
                .decision,
                SessionTradeDecision::Buffered
            );
            let flush = bridge.register_detected_pool(pool, now);
            replay_buffered_canonical_trades(&event_tx, pool, &flush, None, &registry);
            let (tx, receipt) = match event_rx.try_recv().expect("buffered replay emission") {
                GhostEvent::PoolTransaction(tx, Some(permit)) => (tx, permit.apply_receipt),
                other => panic!("buffer replay must retain the original permit, got {other:?}"),
            };
            let result = session.ingest_transaction_with_apply_result(tx);
            assert_eq!(
                result.apply,
                CanonicalMutationApplyOutcomeV1::AppliedNewMutation
            );
            let completion =
                complete_receipt_after_session_apply(&registry, &receipt, result.apply);
            assert_eq!(completion.downstream_applies, 1);
            assert_eq!(completion.ready_publications, 1);
            CrossLayerExecutionV1 {
                canonical_emissions: 1,
                downstream_applies: completion.downstream_applies,
                ready_publications: completion.ready_publications,
                ..CrossLayerExecutionV1::default()
            }
        }
        "buffered_trade_expiry" => {
            let mut expiring_bridge =
                SessionPoolTradeBridge::new(Duration::ZERO, 1, 1, Duration::from_secs(60), 1);
            let permit = primary_permit(
                &ledger,
                &registry,
                primary_trade_observation(signature, pool, mint, 0, Some(1)),
                1,
            );
            let trade = trade_carrier(signature, pool, mint, 0);
            let (event_tx, mut event_rx) = create_event_bus();
            let now = Instant::now();
            assert_eq!(
                process_trade_event_for_session_gate(
                    &event_tx,
                    &mut expiring_bridge,
                    &trade,
                    None,
                    now,
                    permit,
                    &registry,
                )
                .decision,
                SessionTradeDecision::Buffered
            );
            let flush =
                expiring_bridge.register_detected_pool(pool, now + Duration::from_millis(1));
            replay_buffered_canonical_trades(&event_tx, pool, &flush, None, &registry);
            assert!(
                event_rx.try_recv().is_err(),
                "expired permit cannot emit a replay"
            );
            assert!(registry.evaluation_guard(candidate).is_err());
            CrossLayerExecutionV1 {
                difference_class: Some("BUFFERED_CANONICAL_APPLY_EXPIRED"),
                ..CrossLayerExecutionV1::default()
            }
        }
        "account_update_duplicate" => execute_account_state_scenario(false),
        "account_same_version_different_hash" => execute_account_state_scenario(true),
        "continuity_only_restored_position" => {
            let permit = primary_permit(
                &ledger,
                &registry,
                primary_trade_observation(signature, pool, mint, 0, Some(1)),
                1,
            );
            assert_eq!(
                authorize_pool_runtime_disposition(
                    PoolDetectionRuntimeDispositionV1::ContinuityOnly,
                    &permit,
                    registry.as_ref(),
                ),
                Err(CanonicalRuntimeNoApplyReasonV1::ContinuityOnly)
            );
            assert!(registry.evaluation_guard(candidate).is_err());
            CrossLayerExecutionV1::default()
        }
        "queue_saturation" => {
            assert!(handle_local_coverage_gap_notice(
                registry.as_ref(),
                "primary",
                &LocalCoverageGapNoticeV1 {
                    provider_id: "primary".to_string(),
                    reason: ghost_core::LocalCoverageGapReasonV1::IpcEgressQueueSaturated,
                },
            ));
            assert!(matches!(
                ingest_pump_observation(
                    &ledger,
                    &registry,
                    Some(primary_trade_observation(signature, pool, mint, 0, Some(1))),
                    1,
                    true,
                    None,
                ),
                CanonicalRuntimeAdmissionV1::Blocked(
                    CandidateIntegrityOutcomeV1::PrimaryRawCoverageIncomplete
                )
            ));
            CrossLayerExecutionV1 {
                difference_class: Some("PRIMARY_BOUNDARY_INCOMPLETE"),
                ..CrossLayerExecutionV1::default()
            }
        }
        other => panic!("unmapped frozen PR1E scenario: {other}"),
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

#[test]
fn pr1e_frozen_cross_layer_corpus_executes_each_scenario_through_production_adapters() {
    let scenarios = std::str::from_utf8(CROSS_LAYER_BYTES)
        .expect("UTF-8 JSONL")
        .lines()
        .map(|line| serde_json::from_str::<CrossLayerScenarioV1>(line).expect("valid scenario"))
        .collect::<Vec<_>>();
    assert_eq!(
        scenarios.len(),
        23,
        "the frozen corpus is a complete execution inventory"
    );

    for scenario in scenarios {
        let actual = execute_cross_layer_scenario(&scenario.scenario_id);
        assert_eq!(
            actual.canonical_emissions, scenario.expected_canonical_emissions,
            "{}: production canonical emission count",
            scenario.scenario_id
        );
        assert_eq!(
            actual.downstream_applies, scenario.expected_canonical_applies,
            "{}: real session/account downstream apply count",
            scenario.scenario_id
        );
        assert_eq!(
            actual.false_ready, scenario.expected_false_ready,
            "{}: Ready may not be visible before exact downstream apply",
            scenario.scenario_id
        );
        assert_eq!(
            actual.difference_class.map(str::to_owned),
            scenario.difference_class,
            "{}: classified parent-to-enforce difference",
            scenario.scenario_id
        );
        assert_eq!(
            actual.gatekeeper_invocations, 0,
            "{}: qualification never turns a technical integrity path into a policy evaluation",
            scenario.scenario_id
        );
    }
}

#[tokio::test]
async fn pr1e_runner_uses_production_ledger_registry_and_event_adapter() {
    let ledger = Arc::new(Mutex::new(PumpObservationLedgerV1::default()));
    let registry = Arc::new(CandidateIntegrityRegistry::default());
    let pool = Pubkey::new_from_array([1; 32]);
    let mint = Pubkey::new_from_array([2; 32]);
    let signature = Signature::from([3; 64]);
    let observation = primary_trade_observation(signature, pool, mint, 0, Some(1));

    let permit =
        match ingest_pump_observation(&ledger, &registry, Some(observation.clone()), 1, true, None)
        {
            CanonicalRuntimeAdmissionV1::Apply(permit) => permit,
            other => panic!("unique primary must produce one canonical permit: {other:?}"),
        };
    let mut bridge =
        SessionPoolTradeBridge::new(Duration::from_secs(1), 4, 16, Duration::from_secs(60), 32);
    let trade = trade_carrier(signature, pool, mint, 0);
    let mut session = qualification_session(pool, mint);
    let execution = forward_permitted_trade_through_session(
        &registry,
        &mut bridge,
        &mut session,
        &trade,
        permit,
        Instant::now(),
    );
    assert_eq!(execution.canonical_emissions, 1);
    assert_eq!(execution.downstream_applies, 1);
    assert_eq!(execution.ready_publications, 1);

    let candidate = PumpCandidateIdentityV1 {
        pool_amm_id: pool,
        mint,
    };
    assert!(registry.evaluation_guard(candidate).is_ok());

    assert!(matches!(
        ingest_pump_observation(&ledger, &registry, Some(observation.clone()), 2, true, None,),
        CanonicalRuntimeAdmissionV1::NoApply(_)
    ));

    let mut secondary = observation.clone();
    secondary.raw_provider_role = Some(RawProviderRoleV1::SecondaryWitness);
    secondary.provenance.provider_id = "secondary".to_string();
    secondary.provenance.payload_hash_blake3 = [9; 32];
    assert!(matches!(
        ingest_pump_observation(&ledger, &registry, Some(secondary), 3, true, None),
        CanonicalRuntimeAdmissionV1::NoApply(_)
    ));

    let mut nln = observation;
    nln.locator_hint = None;
    nln.canonical_order = None;
    nln.raw_provider_role = None;
    nln.provenance.source_family = ObservationSourceFamilyV1::ParsedNln;
    nln.provenance.provider_id = "nln".to_string();
    nln.provenance.payload_hash_blake3 = [10; 32];
    assert!(matches!(
        ingest_pump_observation(&ledger, &registry, Some(nln), 4, true, None),
        CanonicalRuntimeAdmissionV1::NoApply(_)
    ));

    let snapshot = ledger.lock().expect("ledger").snapshot();
    assert_eq!(snapshot.canonical_mutation_count, 1);
}

#[tokio::test]
async fn pr1e_runner_preserves_multi_locator_inventory_and_witness_arrival_order() {
    let ledger = Arc::new(Mutex::new(PumpObservationLedgerV1::default()));
    let registry = Arc::new(CandidateIntegrityRegistry::default());
    let pool = Pubkey::new_from_array([11; 32]);
    let mint = Pubkey::new_from_array([12; 32]);
    let signature = Signature::from([13; 64]);
    let first = primary_trade_observation(signature, pool, mint, 0, Some(2));
    let second = primary_trade_observation(signature, pool, mint, 1, Some(2));

    let first_permit = match ingest_pump_observation(&ledger, &registry, Some(first), 1, true, None)
    {
        CanonicalRuntimeAdmissionV1::Apply(permit) => permit,
        other => panic!("first locator must retain its own permit: {other:?}"),
    };
    let second_permit =
        match ingest_pump_observation(&ledger, &registry, Some(second), 2, true, None) {
            CanonicalRuntimeAdmissionV1::Apply(permit) => permit,
            other => panic!("second locator must retain its own permit: {other:?}"),
        };

    let candidate = PumpCandidateIdentityV1 {
        pool_amm_id: pool,
        mint,
    };
    let mut bridge =
        SessionPoolTradeBridge::new(Duration::from_secs(1), 4, 16, Duration::from_secs(60), 32);
    let mut session = qualification_session(pool, mint);
    let first_execution = forward_permitted_trade_through_session(
        &registry,
        &mut bridge,
        &mut session,
        &trade_carrier(signature, pool, mint, 0),
        first_permit,
        Instant::now(),
    );
    assert_eq!(first_execution.downstream_applies, 1);
    assert_eq!(first_execution.ready_publications, 0);
    assert!(
        registry.evaluation_guard(candidate).is_err(),
        "one applied locator must not satisfy a two-locator candidate"
    );
    let second_execution = forward_permitted_trade_through_session(
        &registry,
        &mut bridge,
        &mut session,
        &trade_carrier(signature, pool, mint, 1),
        second_permit,
        Instant::now(),
    );
    assert_eq!(second_execution.downstream_applies, 1);
    assert_eq!(second_execution.ready_publications, 1);
    assert!(registry.evaluation_guard(candidate).is_ok());
    assert_eq!(
        ledger
            .lock()
            .expect("multi-locator ledger snapshot")
            .snapshot()
            .canonical_mutation_count,
        2
    );

    let witness_ledger = Arc::new(Mutex::new(PumpObservationLedgerV1::default()));
    let witness_registry = Arc::new(CandidateIntegrityRegistry::default());
    let witness_pool = Pubkey::new_from_array([21; 32]);
    let witness_mint = Pubkey::new_from_array([22; 32]);
    let witness_signature = Signature::from([23; 64]);
    let raw = primary_trade_observation(witness_signature, witness_pool, witness_mint, 0, Some(1));
    let mut nln = raw.clone();
    nln.canonical_order = None;
    nln.raw_provider_role = None;
    nln.provenance.source_family = ObservationSourceFamilyV1::ParsedNln;
    nln.provenance.source_id = "nln".to_string();
    nln.provenance.provider_id = "nln".to_string();
    nln.provenance.schema_id = "nln_pump_trade_v1".to_string();
    nln.provenance.payload_hash_blake3 = [24; 32];

    assert!(matches!(
        ingest_pump_observation(
            &witness_ledger,
            &witness_registry,
            Some(nln.clone()),
            1,
            true,
            None,
        ),
        CanonicalRuntimeAdmissionV1::NoApply(_)
    ));
    let raw_permit =
        match ingest_pump_observation(&witness_ledger, &witness_registry, Some(raw), 2, true, None)
        {
            CanonicalRuntimeAdmissionV1::Apply(permit) => permit,
            other => panic!("NLN-first must not veto later raw primary: {other:?}"),
        };
    let mut witness_bridge =
        SessionPoolTradeBridge::new(Duration::from_secs(1), 4, 16, Duration::from_secs(60), 32);
    let mut witness_session = qualification_session(witness_pool, witness_mint);
    let witness_execution = forward_permitted_trade_through_session(
        &witness_registry,
        &mut witness_bridge,
        &mut witness_session,
        &trade_carrier(witness_signature, witness_pool, witness_mint, 0),
        raw_permit,
        Instant::now(),
    );
    assert_eq!(witness_execution.ready_publications, 1);
    assert!(witness_registry
        .evaluation_guard(PumpCandidateIdentityV1 {
            pool_amm_id: witness_pool,
            mint: witness_mint,
        })
        .is_ok());

    let conflict_ledger = Arc::new(Mutex::new(PumpObservationLedgerV1::default()));
    let conflict_registry = Arc::new(CandidateIntegrityRegistry::default());
    nln.claims.token_amount_units = Some(999);
    nln.provenance.payload_hash_blake3 = [25; 32];
    assert!(matches!(
        ingest_pump_observation(
            &conflict_ledger,
            &conflict_registry,
            Some(nln),
            1,
            true,
            None,
        ),
        CanonicalRuntimeAdmissionV1::NoApply(_)
    ));
    let conflict_raw =
        primary_trade_observation(witness_signature, witness_pool, witness_mint, 0, Some(1));
    let conflict_permit = match ingest_pump_observation(
        &conflict_ledger,
        &conflict_registry,
        Some(conflict_raw),
        2,
        true,
        None,
    ) {
        CanonicalRuntimeAdmissionV1::Apply(permit) => permit,
        other => panic!("conflicted raw primary still owns structural apply: {other:?}"),
    };
    let mut conflict_bridge =
        SessionPoolTradeBridge::new(Duration::from_secs(1), 4, 16, Duration::from_secs(60), 32);
    let mut conflict_session = qualification_session(witness_pool, witness_mint);
    let conflict_execution = forward_permitted_trade_through_session(
        &conflict_registry,
        &mut conflict_bridge,
        &mut conflict_session,
        &trade_carrier(witness_signature, witness_pool, witness_mint, 0),
        conflict_permit,
        Instant::now(),
    );
    assert_eq!(conflict_execution.downstream_applies, 1);
    assert_eq!(conflict_execution.ready_publications, 0);
    assert!(matches!(
        conflict_registry.evaluation_guard(PumpCandidateIdentityV1 {
            pool_amm_id: witness_pool,
            mint: witness_mint,
        }),
        Err(
            crate::candidate_integrity::CandidateIntegrityErrorV1::NotReady(
                CandidateIntegrityOutcomeV1::SourceReconciliationConflict
            )
        )
    ));
    assert_eq!(
        conflict_ledger
            .lock()
            .expect("conflict ledger snapshot")
            .snapshot()
            .canonical_mutation_count,
        1
    );
}
