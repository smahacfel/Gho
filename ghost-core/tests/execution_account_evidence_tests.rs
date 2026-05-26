use ghost_core::{
    ExecutionAccountEvidence, ExecutionAccountEvidenceSource, ExecutionAccountEvidenceStatus,
    ExecutionAccountEvidenceStore, ExecutionAccountRole, UpsertExecutionAccountEvidenceOutcome,
};
use solana_sdk::pubkey::Pubkey;

fn evidence(
    role: ExecutionAccountRole,
    account_pubkey: Pubkey,
    source: ExecutionAccountEvidenceSource,
    status: ExecutionAccountEvidenceStatus,
    received_at_ms: u64,
) -> ExecutionAccountEvidence {
    ExecutionAccountEvidence {
        role,
        account_pubkey,
        base_mint: None,
        pool_id: None,
        canonical_bonding_curve: None,
        source,
        status,
        slot: None,
        context_slot: None,
        write_version: None,
        owner: None,
        data_len: None,
        tx_signature: None,
        observed_instruction_index: None,
        observed_account_position: None,
        provenance_status: None,
        detected_at_ms: received_at_ms,
        received_at_ms,
        evidence_ready: status == ExecutionAccountEvidenceStatus::AccountUpdateReceived
            || status == ExecutionAccountEvidenceStatus::RpcReady
            || status == ExecutionAccountEvidenceStatus::PrecheckReady,
        reason: None,
    }
}

#[test]
fn execution_account_evidence_store_records_under_exact_role_pubkey() {
    let store = ExecutionAccountEvidenceStore::new();
    let bcv2_pubkey = Pubkey::new_unique();
    let creator_vault = Pubkey::new_unique();

    let result = store.upsert(evidence(
        ExecutionAccountRole::BondingCurveV2,
        bcv2_pubkey,
        ExecutionAccountEvidenceSource::ObservedTxMeta,
        ExecutionAccountEvidenceStatus::DiscoveryHint,
        1,
    ));

    assert_eq!(
        result.outcome,
        UpsertExecutionAccountEvidenceOutcome::Inserted
    );
    assert_eq!(result.role, ExecutionAccountRole::BondingCurveV2);
    assert_eq!(result.account_pubkey, bcv2_pubkey);
    assert_eq!(
        result.best_positive_status,
        Some(ExecutionAccountEvidenceStatus::DiscoveryHint)
    );

    assert!(store
        .get(ExecutionAccountRole::BondingCurveV2, &bcv2_pubkey)
        .is_some());
    assert!(store
        .get(ExecutionAccountRole::CreatorVault, &bcv2_pubkey)
        .is_none());
    assert!(store
        .get(ExecutionAccountRole::BondingCurveV2, &creator_vault)
        .is_none());
}

#[test]
fn execution_account_evidence_bonding_curve_v2_does_not_collapse_to_canonical_bonding_curve() {
    let store = ExecutionAccountEvidenceStore::new();
    let bcv2_pubkey = Pubkey::new_unique();
    let canonical_bonding_curve = Pubkey::new_unique();
    let mut row = evidence(
        ExecutionAccountRole::BondingCurveV2,
        bcv2_pubkey,
        ExecutionAccountEvidenceSource::YellowstoneAccountUpdate,
        ExecutionAccountEvidenceStatus::AccountUpdateReceived,
        1,
    );
    row.canonical_bonding_curve = Some(canonical_bonding_curve);

    store.upsert(row);

    assert!(store
        .get(ExecutionAccountRole::BondingCurveV2, &bcv2_pubkey)
        .is_some());
    assert!(store
        .get(
            ExecutionAccountRole::BondingCurveV2,
            &canonical_bonding_curve
        )
        .is_none());
    assert!(store
        .get(
            ExecutionAccountRole::Other("bonding_curve".to_string()),
            &bcv2_pubkey
        )
        .is_none());
}

#[test]
fn execution_account_evidence_rpc_ready_replaces_discovery_hint_as_best_positive() {
    let store = ExecutionAccountEvidenceStore::new();
    let bcv2_pubkey = Pubkey::new_unique();

    store.upsert(evidence(
        ExecutionAccountRole::BondingCurveV2,
        bcv2_pubkey,
        ExecutionAccountEvidenceSource::ObservedTxMeta,
        ExecutionAccountEvidenceStatus::DiscoveryHint,
        1,
    ));
    let result = store.upsert(evidence(
        ExecutionAccountRole::BondingCurveV2,
        bcv2_pubkey,
        ExecutionAccountEvidenceSource::RpcHydration,
        ExecutionAccountEvidenceStatus::RpcReady,
        2,
    ));

    assert_eq!(
        result.outcome,
        UpsertExecutionAccountEvidenceOutcome::Updated
    );
    assert_eq!(
        result.best_positive_status,
        Some(ExecutionAccountEvidenceStatus::RpcReady)
    );

    let record = store
        .get(ExecutionAccountRole::BondingCurveV2, &bcv2_pubkey)
        .expect("record should exist");
    assert_eq!(
        record.best_positive.as_ref().map(|row| row.status),
        Some(ExecutionAccountEvidenceStatus::RpcReady)
    );
    assert_eq!(
        record.best_positive.as_ref().map(|row| row.source),
        Some(ExecutionAccountEvidenceSource::RpcHydration)
    );
}

#[test]
fn execution_account_evidence_rpc_missing_does_not_remove_account_update_positive_evidence() {
    let store = ExecutionAccountEvidenceStore::new();
    let bcv2_pubkey = Pubkey::new_unique();

    store.upsert(evidence(
        ExecutionAccountRole::BondingCurveV2,
        bcv2_pubkey,
        ExecutionAccountEvidenceSource::YellowstoneAccountUpdate,
        ExecutionAccountEvidenceStatus::AccountUpdateReceived,
        1,
    ));
    let result = store.upsert(evidence(
        ExecutionAccountRole::BondingCurveV2,
        bcv2_pubkey,
        ExecutionAccountEvidenceSource::RpcHydration,
        ExecutionAccountEvidenceStatus::RpcMissing,
        2,
    ));

    assert_eq!(
        result.best_positive_status,
        Some(ExecutionAccountEvidenceStatus::AccountUpdateReceived)
    );
    assert_eq!(
        result.latest_negative_status,
        Some(ExecutionAccountEvidenceStatus::RpcMissing)
    );
    assert!(result.conflict.is_some());

    let record = store
        .get(ExecutionAccountRole::BondingCurveV2, &bcv2_pubkey)
        .expect("record should exist");
    assert_eq!(
        record.best_positive.as_ref().map(|row| row.status),
        Some(ExecutionAccountEvidenceStatus::AccountUpdateReceived)
    );
    assert_eq!(
        record.latest_negative.as_ref().map(|row| row.status),
        Some(ExecutionAccountEvidenceStatus::RpcMissing)
    );
    assert!(record.conflict.is_some());
}

#[test]
fn execution_account_evidence_helper_indexes_return_exact_bcv2_pubkeys_without_duplicates() {
    let store = ExecutionAccountEvidenceStore::new();
    let base_mint = Pubkey::new_unique();
    let pool_id = Pubkey::new_unique();
    let bcv2_pubkey = Pubkey::new_unique();
    let mut row = evidence(
        ExecutionAccountRole::BondingCurveV2,
        bcv2_pubkey,
        ExecutionAccountEvidenceSource::ObservedTxMeta,
        ExecutionAccountEvidenceStatus::DiscoveryHint,
        1,
    );
    row.base_mint = Some(base_mint);
    row.pool_id = Some(pool_id);

    store.upsert(row.clone());
    row.status = ExecutionAccountEvidenceStatus::RpcReady;
    row.source = ExecutionAccountEvidenceSource::RpcHydration;
    row.received_at_ms = 2;
    row.evidence_ready = true;
    store.upsert(row);

    assert_eq!(
        store.find_by_base_mint_role(&base_mint, ExecutionAccountRole::BondingCurveV2),
        vec![bcv2_pubkey]
    );
    assert_eq!(
        store.find_by_pool_role(&pool_id, ExecutionAccountRole::BondingCurveV2),
        vec![bcv2_pubkey]
    );
    assert!(store
        .find_by_base_mint_role(&base_mint, ExecutionAccountRole::CreatorVault)
        .is_empty());
}

#[test]
fn execution_account_evidence_role_source_and_status_use_canonical_json_labels() {
    let role = ExecutionAccountRole::BondingCurveV2;
    assert_eq!(
        serde_json::to_string(&role).expect("role should serialize"),
        "\"bonding_curve_v2\""
    );
    assert_eq!(
        serde_json::from_str::<ExecutionAccountRole>("\"other:custom_role\"")
            .expect("other role should deserialize"),
        ExecutionAccountRole::Other("custom_role".to_string())
    );
    assert_eq!(
        serde_json::to_string(&ExecutionAccountEvidenceSource::RpcHydration)
            .expect("source should serialize"),
        "\"rpc_hydration\""
    );
    assert_eq!(
        serde_json::to_string(&ExecutionAccountEvidenceStatus::AccountUpdateReceived)
            .expect("status should serialize"),
        "\"account_update_received\""
    );
}

#[test]
fn execution_account_evidence_snapshot_counts_reflect_latest_statuses_and_conflicts() {
    let store = ExecutionAccountEvidenceStore::new();
    let bcv2_pubkey = Pubkey::new_unique();
    let creator_vault = Pubkey::new_unique();

    store.upsert(evidence(
        ExecutionAccountRole::BondingCurveV2,
        bcv2_pubkey,
        ExecutionAccountEvidenceSource::YellowstoneAccountUpdate,
        ExecutionAccountEvidenceStatus::AccountUpdateReceived,
        1,
    ));
    store.upsert(evidence(
        ExecutionAccountRole::BondingCurveV2,
        bcv2_pubkey,
        ExecutionAccountEvidenceSource::RpcHydration,
        ExecutionAccountEvidenceStatus::RpcMissing,
        2,
    ));
    store.upsert(evidence(
        ExecutionAccountRole::CreatorVault,
        creator_vault,
        ExecutionAccountEvidenceSource::RpcPrecheck,
        ExecutionAccountEvidenceStatus::PrecheckReady,
        3,
    ));

    let counts = store.snapshot_counts();
    assert_eq!(counts.total_records, 2);
    assert_eq!(counts.positive_records, 2);
    assert_eq!(counts.negative_records, 1);
    assert_eq!(counts.conflict_records, 1);
    assert_eq!(counts.evidence_ready_records, 2);
    assert_eq!(counts.latest_status_counts.get("rpc_missing"), Some(&1));
    assert_eq!(counts.latest_status_counts.get("precheck_ready"), Some(&1));
    assert_eq!(counts.latest_source_counts.get("rpc_hydration"), Some(&1));
    assert_eq!(counts.latest_source_counts.get("rpc_precheck"), Some(&1));
}
