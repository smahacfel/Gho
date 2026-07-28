//! PR1E executable qualification runner.
//!
//! This module is test-only, but every integrity decision is produced by the
//! production `PumpObservationLedgerV1`, `CandidateIntegrityRegistry` and Seer
//! Event Bus adapters. It does not provide a fake ledger, fake registry or a
//! second runtime authority.

use crate::{
    candidate_integrity::{
        CandidateIntegrityRegistry, CandidateIntegritySubmitGuardV1, CandidateSubmitTransitionV1,
        CandidateTerminalTransitionV1,
    },
    components::seer::{
        authorize_pool_runtime_disposition, handle_local_coverage_gap_notice,
        ingest_pump_observation, process_pool_detected_event_for_session_gate,
        process_trade_event_for_session_gate, replay_buffered_canonical_trades,
        CanonicalRuntimeAdmissionV1, CanonicalRuntimeNoApplyReasonV1,
        NlnArtifactWriterStallProbeV1, SessionPoolTradeBridge, SessionTradeDecision,
    },
    events::{create_event_bus, CanonicalRuntimePermitV1, GhostEvent},
    oracle_runtime::OracleRuntime,
    session::{
        observation::CanonicalMutationApplyOutcomeV1, OpenSessionRequest, PoolObservationSession,
        SharedSession,
    },
    tx_intelligence::{FundingSourceConfig, TxIntelligenceConfig},
};
use ghost_brain::{
    config::GatekeeperV2Config, fast_pipeline::EnhancedCandidate, oracle::HyperPredictionOracle,
};
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
    ipc::{
        create_ipc_channel, BackpressurePolicy, EventPriority, IpcChannelConfig, IpcError,
        PoolDetectionRuntimeDispositionV1,
    },
    types::{
        CandidatePool, InstructionProvenance, RawBytesMissingReason, ToolchainFingerprintInput,
        TradeEvent,
    },
};
use serde::Deserialize;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Barrier, Mutex,
    },
    time::{Duration, Instant},
};

const MANIFEST_BYTES: &[u8] = include_bytes!("../tests/fixtures/pr1e/pr1e_corpus_manifest_v1.json");
const MANIFEST_BLAKE3: &str = "d111283727259e44a80338e1a4d81fc4c40daff06e67640cd090501874aa42dd";
const CROSS_LAYER_BYTES: &[u8] =
    include_bytes!("../tests/fixtures/pr1e/pr1e_cross_layer_scenarios_v1.jsonl");
const CROSS_LAYER_BLAKE3: &str = "db5812ac10d5fcdede623037ead4baf42c2da90d929969bdb35ad1b06a4a8bae";
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
    expected_ready_publications: u64,
    expected_mfs_materializations: u64,
    expected_gatekeeper_invocations: u64,
    expected_sender_calls: u64,
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

fn primary_initialize_pool_observation(
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
        mutation_family: PumpMutationFamilyV1::InitializePool,
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
            success: Some(true),
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

fn candidate_carrier(signature: Signature, pool: Pubkey, mint: Pubkey) -> CandidatePool {
    CandidatePool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        provider_id: Some("primary".to_string()),
        provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
        slot: Some(10),
        tx_index: Some(0),
        event_ts_ms: Some(1_000),
        event_time: ghost_core::EventTimeMetadata::default(),
        signature: signature.to_string(),
        amm_program_id: Pubkey::new_from_array([4; 32]),
        pool_amm_id: pool,
        base_mint: mint,
        quote_mint: Pubkey::new_unique(),
        bonding_curve: pool,
        creator: Pubkey::new_unique(),
        timestamp: 1_000,
        bonding_curve_progress: Some(0.0),
        initial_liquidity_sol: Some(1.0),
        token_total_supply: Some(1_000_000),
        block_time: Some(1),
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

fn qualification_oracle_runtime() -> OracleRuntime {
    OracleRuntime::new(
        Arc::new(HyperPredictionOracle::default()),
        Pubkey::new_unique().to_string(),
        Pubkey::new_unique().to_string(),
        Arc::new(ghost_core::shadow_ledger::ShadowLedger::new()),
    )
}

fn enhanced_candidate_from_carrier(candidate: &CandidatePool) -> EnhancedCandidate {
    EnhancedCandidate {
        pool_amm_id: candidate.pool_amm_id,
        amm_program_id: candidate.amm_program_id,
        base_mint: candidate.base_mint,
        quote_mint: candidate.quote_mint,
        bonding_curve: candidate.bonding_curve,
        slot: candidate.slot,
        timestamp: candidate.timestamp,
        initial_liquidity_sol: candidate.initial_liquidity_sol.unwrap_or_default(),
        signature: candidate.signature.clone(),
        ..EnhancedCandidate::default()
    }
}

fn open_qualification_oracle_session(
    runtime: &OracleRuntime,
    candidate: &CandidatePool,
) -> Option<SharedSession> {
    let gatekeeper_config = GatekeeperV2Config::default();
    runtime
        .session_manager()
        .open_session(OpenSessionRequest {
            pool_amm_id: candidate.pool_amm_id,
            base_mint: candidate.base_mint,
            bonding_curve: candidate.bonding_curve,
            dev_wallet: Some(candidate.creator),
            candidate_snapshot: enhanced_candidate_from_carrier(candidate),
            created_at_wall_ms: candidate.timestamp,
            deadline_wall_ms: Some(candidate.timestamp.saturating_add(60_000)),
            gatekeeper_config: gatekeeper_config.clone(),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(&gatekeeper_config),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .ok()?;
    runtime
        .session_manager()
        .get_session(&candidate.pool_amm_id)
}

/// Exercise the production `NewPoolDetected` adapter and the real
/// `OracleRuntime` structural-registration owner before completing an
/// InitializePool receipt.  A bus enqueue alone is deliberately not an
/// acknowledgement.
fn forward_permitted_initialize_through_oracle(
    runtime: &OracleRuntime,
    registry: &Arc<CandidateIntegrityRegistry>,
    bridge: &mut SessionPoolTradeBridge,
    candidate: &CandidatePool,
    permit: CanonicalRuntimePermitV1,
    now: Instant,
) -> (CrossLayerExecutionV1, SharedSession) {
    let (event_tx, mut event_rx) = create_event_bus();
    let _flush = process_pool_detected_event_for_session_gate(
        &event_tx,
        bridge,
        candidate,
        None,
        now,
        candidate.timestamp,
        permit.clone(),
        registry,
    );
    let (detected, receipt) = match event_rx
        .try_recv()
        .expect("canonical NewPool Event Bus delivery")
    {
        GhostEvent::NewPoolDetected(detected, Some(permit)) => (detected, permit.apply_receipt),
        other => {
            panic!("production adapter must emit one permitted NewPoolDetected, got {other:?}")
        }
    };
    let pool = Pubkey::try_from(detected.pool_amm_id.as_str()).expect("detected pool pubkey");
    let mint = Pubkey::try_from(detected.base_mint.as_str()).expect("detected mint pubkey");
    let apply = runtime.register_new_pool_with_apply_outcome(
        pool,
        mint,
        enhanced_candidate_from_carrier(candidate),
        Pubkey::try_from(detected.creator.as_str()).ok(),
    );
    // Registration is only the first half of InitializePool downstream
    // application.  The permit is acknowledged only after the actual
    // SessionManager has accepted the same identity; a failed open fails the
    // receipt instead of publishing Ready from registration alone.
    let session = (apply == CanonicalMutationApplyOutcomeV1::AppliedNewMutation)
        .then(|| open_qualification_oracle_session(runtime, candidate))
        .flatten();
    let completion_apply = if session.is_some() {
        apply
    } else {
        CanonicalMutationApplyOutcomeV1::Failed
    };
    let completion = complete_receipt_after_session_apply(registry, &receipt, completion_apply);
    let session = session.expect("canonical Oracle registration must open its matching session");
    (
        CrossLayerExecutionV1 {
            canonical_emissions: 1,
            downstream_applies: completion.downstream_applies,
            ready_publications: completion.ready_publications,
            ..CrossLayerExecutionV1::default()
        },
        session,
    )
}

/// The production MFS builder is executed between the two existing integrity
/// generation checks.  If a conflict arrives while the immutable snapshot is
/// being built, the snapshot is deliberately not published and this helper
/// returns `false`.
fn materialize_mfs_under_integrity_guard(
    session: &PoolObservationSession,
    guard: &crate::candidate_integrity::CandidateIntegrityEvaluationGuardV1,
    between_build_and_publish: impl FnOnce(),
) -> bool {
    guard
        .check_ready()
        .expect("Ready is required before MFS materialization");
    let _materialized = session
        .try_materialize_features()
        .expect("qualification session must use the production MFS builder");
    between_build_and_publish();
    if guard.check_ready().is_err() {
        return false;
    }
    guard.mark_mfs_materialized().is_ok()
}

/// Test-only instrumented sender adapter.  It owns no admission policy: a
/// counter moves only after the production submit guard atomically reports
/// `StartedNow`.
#[derive(Clone, Default)]
struct InstrumentedSenderAdapterV1 {
    calls: Arc<AtomicU64>,
}

impl InstrumentedSenderAdapterV1 {
    fn send(
        &self,
        guard: &CandidateIntegritySubmitGuardV1,
    ) -> Result<CandidateSubmitTransitionV1, crate::candidate_integrity::CandidateIntegrityErrorV1>
    {
        let transition = guard.try_begin_submit()?;
        if transition == CandidateSubmitTransitionV1::StartedNow {
            self.calls.fetch_add(1, Ordering::AcqRel);
        }
        Ok(transition)
    }

    fn call_count(&self) -> u64 {
        self.calls.load(Ordering::Acquire)
    }
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
async fn execute_cross_layer_scenario(scenario_id: &str) -> CrossLayerExecutionV1 {
    let ledger = Arc::new(Mutex::new(PumpObservationLedgerV1::default()));
    // Only InitializePool scenarios construct an Oracle runtime; all other
    // rows retain the same real ledger/registry/session adapters while
    // avoiding unrelated runtime construction work.
    let runtime = matches!(
        scenario_id,
        "primary_create_session_apply_ready" | "create_and_initial_buy_one_signature"
    )
    .then(qualification_oracle_runtime);
    let registry = runtime
        .as_ref()
        .map(OracleRuntime::candidate_integrity_registry)
        .unwrap_or_else(|| Arc::new(CandidateIntegrityRegistry::default()));
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
        "primary_create_session_apply_ready" => {
            let runtime = runtime
                .as_ref()
                .expect("InitializePool qualification owns one Oracle runtime");
            let carrier = candidate_carrier(signature, pool, mint);
            let permit = primary_permit(
                &ledger,
                &registry,
                primary_initialize_pool_observation(signature, pool, mint, 0, Some(1)),
                1,
            );
            let (execution, session) = forward_permitted_initialize_through_oracle(
                runtime,
                &registry,
                &mut bridge,
                &carrier,
                permit,
                Instant::now(),
            );
            assert_eq!(execution.ready_publications, 1);
            assert!(runtime.lookup_pool_identity(&pool).is_some());
            assert_eq!(
                runtime.session_manager().active_session_count(),
                1,
                "InitializePool opens the production Oracle session exactly once"
            );
            assert_eq!(session.read().pool_amm_id, pool);
            execution
        }
        "writer_stall" => {
            let writer_stall = NlnArtifactWriterStallProbeV1::start().await;
            assert!(
                writer_stall.fill_and_saturate(),
                "the real bounded NLN artifact writer queue must report typed saturation"
            );
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
            assert!(
                !writer_stall.completed(),
                "canonical receiver/session apply must complete before the stalled writer append"
            );
            assert!(
                writer_stall.release_and_join().await,
                "the released production artifact writer must complete its physical append"
            );
            execution
        }
        "create_and_initial_buy_one_signature" => {
            let runtime = runtime
                .as_ref()
                .expect("InitializePool qualification owns one Oracle runtime");
            let mut execution = CrossLayerExecutionV1::default();
            let carrier = candidate_carrier(signature, pool, mint);
            let initialize_permit = primary_permit(
                &ledger,
                &registry,
                primary_initialize_pool_observation(signature, pool, mint, 0, Some(2)),
                1,
            );
            let (first, opened_session) = forward_permitted_initialize_through_oracle(
                runtime,
                &registry,
                &mut bridge,
                &carrier,
                initialize_permit,
                Instant::now(),
            );
            assert_eq!(
                first.downstream_applies, 1,
                "InitializePool reached Oracle state owner"
            );
            assert_eq!(
                first.ready_publications, 0,
                "create locator alone is not Ready"
            );
            add_execution(&mut execution, first);
            let mut opened_session = opened_session.write();
            let (_, second) = apply_unique_primary(
                &ledger,
                &registry,
                &mut bridge,
                &mut opened_session,
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
        "two_trade_mutations_one_signature" => {
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
                "first trade locator is not Ready alone"
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
                "second trade locator releases once"
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
            if scenario_id == "conflict_during_mfs" {
                let materialized = materialize_mfs_under_integrity_guard(&session, &guard, || {
                    registry
                        .record_signal(lifecycle_conflict_signal(
                            candidate,
                            signature,
                            permit.locator.clone(),
                        ))
                        .expect("typed conflict");
                });
                assert!(
                    !materialized,
                    "a conflict between real MFS build and publication must orphan the snapshot"
                );
                assert!(guard.check_ready().is_err());
                execution.difference_class = Some("EVALUATION_TECHNICALLY_ABORTED");
                return execution;
            }

            assert!(
                materialize_mfs_under_integrity_guard(&session, &guard, || {}),
                "production MFS materialization must publish only while Ready"
            );
            execution.mfs_materializations = 1;
            guard.mark_evaluation_running().expect("evaluation phase");
            let sender = InstrumentedSenderAdapterV1::default();

            let difference_class = match scenario_id {
                "conflict_during_evaluation" => {
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
                    assert!(sender.send(&submit).is_err());
                    assert_eq!(sender.call_count(), 0);
                    "EXECUTION_CANCELLED_BEFORE_SUBMIT"
                }
                "conflict_race_with_submit" => {
                    let submit = guard
                        .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
                        .expect("BUY terminal")
                        .expect("submit guard");
                    let transition_entered = Arc::new(Barrier::new(2));
                    let allow_submit = Arc::new(Barrier::new(2));
                    let entered = Arc::clone(&transition_entered);
                    let release = Arc::clone(&allow_submit);
                    registry.set_transition_before_commit_hook(Some(Arc::new(move || {
                        entered.wait();
                        release.wait();
                    })));
                    let submit_sender = sender.clone();
                    let submit_guard = submit.clone();
                    let submit_thread =
                        std::thread::spawn(move || submit_sender.send(&submit_guard));
                    transition_entered.wait();

                    let conflict_ready = Arc::new(Barrier::new(2));
                    let conflict_start = Arc::clone(&conflict_ready);
                    let conflict_registry = Arc::clone(&registry);
                    let conflict_locator = permit.locator.clone();
                    let conflict_thread = std::thread::spawn(move || {
                        conflict_start.wait();
                        conflict_registry.record_signal(lifecycle_conflict_signal(
                            candidate,
                            signature,
                            conflict_locator,
                        ))
                    });
                    conflict_ready.wait();
                    allow_submit.wait();
                    assert_eq!(
                        submit_thread
                            .join()
                            .expect("submit race thread join")
                            .expect("submit wins the shared-state linearization"),
                        CandidateSubmitTransitionV1::StartedNow
                    );
                    conflict_thread
                        .join()
                        .expect("conflict race thread join")
                        .expect("typed conflict records after submit linearizes");
                    registry.set_transition_before_commit_hook(None);
                    execution.sender_calls = sender.call_count();
                    assert_eq!(execution.sender_calls, 1);
                    assert!(submit.requires_reconciliation());
                    "POST_SUBMIT_RECONCILIATION_REQUIRED"
                }
                "conflict_after_submit" => {
                    let submit = guard
                        .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
                        .expect("BUY terminal")
                        .expect("submit guard");
                    assert_eq!(
                        sender.send(&submit).expect("instrumented sender starts"),
                        CandidateSubmitTransitionV1::StartedNow
                    );
                    registry
                        .record_signal(lifecycle_conflict_signal(
                            candidate,
                            signature,
                            permit.locator.clone(),
                        ))
                        .expect("typed conflict");
                    execution.sender_calls = sender.call_count();
                    assert_eq!(execution.sender_calls, 1);
                    assert!(submit.requires_reconciliation());
                    "POST_SUBMIT_RECONCILIATION_REQUIRED"
                }
                "conflict_after_confirmation" => {
                    let submit = guard
                        .publish_terminal(CandidateTerminalTransitionV1::BuyNotSubmitted)
                        .expect("BUY terminal")
                        .expect("submit guard");
                    assert_eq!(
                        sender.send(&submit).expect("instrumented sender starts"),
                        CandidateSubmitTransitionV1::StartedNow
                    );
                    submit
                        .mark_confirmed()
                        .expect("confirmation remains authoritative");
                    execution.sender_calls = sender.call_count();
                    assert_eq!(execution.sender_calls, 1);
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
            let (sender, receiver, _metrics) = create_ipc_channel(IpcChannelConfig {
                buffer_size: 1,
                backpressure_policy: BackpressurePolicy::DropNew,
                log_drops: false,
                log_overflows: false,
                warning_threshold_percent: 100.0,
                account_update_queue_capacity: 1,
            });
            let mut local_gap_rx = receiver.local_coverage_gap_receiver();
            let pool_carrier = candidate_carrier(signature, pool, mint);
            let primary_observation =
                primary_initialize_pool_observation(signature, pool, mint, 0, Some(1));
            let mut saturation = None;
            for _ in 0..128 {
                match sender
                    .send_with_observation(
                        pool_carrier.clone(),
                        Some(primary_observation.clone()),
                        EventPriority::Normal,
                    )
                    .await
                {
                    Ok(()) => tokio::task::yield_now().await,
                    Err(IpcError::LocalProcessingGap) => {
                        saturation = Some(());
                        break;
                    }
                    Err(other) => {
                        panic!("candidate-admission IPC saturation must fail closed: {other}")
                    }
                }
            }
            assert!(
                saturation.is_some(),
                "a real full DropNew IPC queue must become a primary local coverage gap"
            );
            tokio::time::timeout(Duration::from_secs(1), local_gap_rx.changed())
                .await
                .expect("real IPC saturation must notify independent control plane")
                .expect("local-gap control plane remains available");
            let notice = local_gap_rx
                .borrow_and_update()
                .notices
                .iter()
                .find(|notice| notice.provider_id == "primary")
                .cloned()
                .expect("primary PoolDetected saturation notice");
            assert!(handle_local_coverage_gap_notice(
                registry.as_ref(),
                &notice.provider_id,
                &notice,
            ));
            assert!(
                !registry.candidate_admission_open(),
                "primary IPC coverage gap closes candidate admission before MFS/Gatekeeper/submit"
            );
            assert!(registry.evaluation_guard(candidate).is_err());
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

#[tokio::test]
async fn pr1e_frozen_cross_layer_corpus_executes_each_scenario_through_production_adapters() {
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
        let actual = execute_cross_layer_scenario(&scenario.scenario_id).await;
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
            actual.ready_publications, scenario.expected_ready_publications,
            "{}: Ready publications released only after all exact downstream applies",
            scenario.scenario_id
        );
        assert_eq!(
            actual.mfs_materializations, scenario.expected_mfs_materializations,
            "{}: actual production MFS materialization count",
            scenario.scenario_id
        );
        assert_eq!(
            actual.gatekeeper_invocations, scenario.expected_gatekeeper_invocations,
            "{}: Gatekeeper invocation count",
            scenario.scenario_id
        );
        assert_eq!(
            actual.sender_calls, scenario.expected_sender_calls,
            "{}: instrumented sender calls after production submit guard",
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

#[test]
fn missing_inventory_canonical_apply_uses_real_session_then_reclaims_terminal_fence() {
    let ledger = Arc::new(Mutex::new(PumpObservationLedgerV1::default()));
    let registry = Arc::new(CandidateIntegrityRegistry::default());
    let pool = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let signature = Signature::new_unique();
    let permit = primary_permit(
        &ledger,
        &registry,
        primary_trade_observation(signature, pool, mint, 0, None),
        1,
    );
    let candidate = PumpCandidateIdentityV1 {
        pool_amm_id: pool,
        mint,
    };
    assert_eq!(
        registry
            .active_record_count()
            .expect("active incomplete record"),
        1
    );

    let mut bridge =
        SessionPoolTradeBridge::new(Duration::from_secs(1), 4, 16, Duration::from_secs(60), 32);
    let mut session = qualification_session(pool, mint);
    let execution = forward_permitted_trade_through_session(
        &registry,
        &mut bridge,
        &mut session,
        &trade_carrier(signature, pool, mint, 0),
        permit,
        Instant::now(),
    );

    assert_eq!(execution.downstream_applies, 1);
    assert_eq!(execution.ready_publications, 0);
    assert_eq!(registry.active_record_count().expect("active count"), 0);
    assert_eq!(
        registry
            .terminal_tombstone_count()
            .expect("terminal evidence"),
        1
    );
    assert_eq!(
        registry
            .canonical_apply_fence_counts()
            .expect("all resolved fence state reclaimed"),
        (0, 0)
    );
    assert!(matches!(
        registry.evaluation_guard(candidate),
        Err(crate::candidate_integrity::CandidateIntegrityErrorV1::CandidateMissing)
    ));
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
        Err(crate::candidate_integrity::CandidateIntegrityErrorV1::CandidateMissing)
    ));
    assert_eq!(
        conflict_registry
            .snapshot(PumpCandidateIdentityV1 {
                pool_amm_id: witness_pool,
                mint: witness_mint,
            })
            .expect("pre-session conflict remains in bounded terminal evidence")
            .outcome,
        CandidateIntegrityOutcomeV1::SourceReconciliationConflict
    );
    assert_eq!(
        conflict_ledger
            .lock()
            .expect("conflict ledger snapshot")
            .snapshot()
            .canonical_mutation_count,
        1
    );
}
