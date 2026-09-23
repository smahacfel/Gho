use ghost_core::tx_intelligence::types::*;

fn ftdi() -> FtdiEvidenceV2 {
    FtdiEvidenceV2 {
        definition: FtdiDefinitionV2::GiniSimpson,
        fee_topology_diversity_index: Some(0.8),
        coordination_hhi: Some(0.2),
        unique_topology_count: 5,
        buy_sample_count: 100,
        signer_sample_count: 100,
        represented_signer_count: 100,
        degraded_reasons: vec![],
    }
}

#[test]
fn m6_ftdi_contract_checks_formula_population_and_explicit_version() {
    let full = ftdi();
    assert!(full.has_full_quality());
    let bytes = serde_json::to_vec(&full).unwrap();
    assert_eq!(
        serde_json::from_slice::<FtdiEvidenceV2>(&bytes).unwrap(),
        full
    );
    let mut wrong = full.clone();
    wrong.fee_topology_diversity_index = Some(0.05); // historyczne K/N
    assert!(wrong.validate().is_err());
    wrong = full.clone();
    wrong.represented_signer_count = 99;
    assert!(wrong.validate().is_err());
    wrong = full.clone();
    wrong.unique_topology_count = 1;
    assert!(wrong.validate().is_err()); // HHI=.2 niemożliwe dla jednej klasy
    wrong = full.clone();
    wrong.signer_sample_count = 101;
    assert!(wrong.validate().is_err());
    let mut json = serde_json::to_value(&full).unwrap();
    json["definition"] = "unique_buyer_actionability_v2".into();
    assert!(serde_json::from_value::<FtdiEvidenceV2>(json).is_err());
}

#[test]
fn m6_ftdi_unknown_partial_and_low_sample_are_not_clean_zero() {
    let mut partial = ftdi();
    partial
        .degraded_reasons
        .push("FTDI_INPUT_STATUS_UNAVAILABLE".into());
    assert!(partial.validate().is_ok());
    assert!(!partial.has_full_quality());
    partial.fee_topology_diversity_index = None;
    partial.coordination_hhi = None;
    assert!(partial.validate().is_ok());
    assert!(!partial.has_full_quality());
    let mut low = ftdi();
    low.buy_sample_count = 2;
    low.signer_sample_count = 2;
    low.represented_signer_count = 2;
    low.unique_topology_count = 1;
    low.coordination_hhi = Some(1.0);
    low.fee_topology_diversity_index = Some(0.0);
    assert!(low.validate().is_ok());
    assert!(!low.has_full_quality());
    low.buy_sample_count = u64::MAX;
    low.signer_sample_count = u64::MAX;
    low.represented_signer_count = u64::MAX;
    assert!(low.has_full_quality()); // u128 granice nie przepełniają się
}

#[test]
fn m6_dbia_sfd_quality_requires_valid_value_and_complete_population() {
    let mut dbia = DbiaEvidenceV1 {
        dev_buyer_infrastructure_affinity: Some(1.0),
        buy_sample_count: 5,
        signer_sample_count: 5,
        represented_signer_count: 5,
        degraded_reasons: vec![],
    };
    let mut sfd = SfdEvidenceV1 {
        spend_fraction_divergence: Some(0.0),
        buy_sample_count: 5,
        signer_sample_count: 5,
        represented_signer_count: 5,
        degraded_reasons: vec![],
    };
    assert!(dbia.has_full_quality() && sfd.has_full_quality());
    dbia.represented_signer_count = 4;
    sfd.represented_signer_count = 4;
    assert!(!dbia.has_full_quality() && !sfd.has_full_quality());
    dbia.represented_signer_count = 5;
    sfd.represented_signer_count = 5;
    dbia.dev_buyer_infrastructure_affinity = Some(f64::NAN);
    sfd.spend_fraction_divergence = Some(1.9);
    assert!(!dbia.has_full_quality() && !sfd.has_full_quality());
}

#[test]
fn m6_des_validator_preserves_none_and_rejects_fake_support() {
    let mut des = DesEvidenceV2 {
        definition: DesDefinitionV2::NextBuySlotTauB,
        interval_unit: DesIntervalUnitV2::Slots,
        price_source: DesPriceSourceV2::PumpVirtualPostTradeReserves,
        demand_elasticity_score: Some(1.0),
        buy_sample_count: 5,
        signer_sample_count: 5,
        priced_buy_count: 5,
        candidate_triple_count: 3,
        closed_triple_count: 3,
        degraded_reasons: vec![],
    };
    assert!(des.has_full_quality());
    des.closed_triple_count = 1;
    assert!(des.validate().is_err());
    des.demand_elasticity_score = None;
    assert!(des.validate().is_ok());
    assert!(!des.has_full_quality());
    des.closed_triple_count = 4;
    assert!(des.validate().is_err());
}

#[test]
fn m6_legacy_scalar_records_remain_legacy_not_fabricated_evidence() {
    let old: SybilResistanceFeatures = serde_json::from_str(
        r#"{"fee_topology_diversity_index":0.05,"demand_elasticity_score":0.9}"#,
    )
    .unwrap();
    assert_eq!(old.fee_topology_diversity_index, Some(0.05));
    assert_eq!(old.demand_elasticity_score, Some(0.9));
    assert!(old.fee_topology_diversity_v2.is_none() && old.demand_elasticity_v2.is_none());
    assert!(old.dbia_evidence_v1.is_none() && old.sfd_evidence_v1.is_none());
    assert!(old.measurement_cutoff_received_ms.is_none());
}

#[test]
fn m6_fraction_evidence_json_roundtrip_preserves_exact_measurement_bits() {
    let evidence = SfdEvidenceV1 {
        spend_fraction_divergence: Some(0.19999999999999998),
        buy_sample_count: 3,
        signer_sample_count: 3,
        represented_signer_count: 3,
        degraded_reasons: vec![],
    };
    let bytes = serde_json::to_vec(&evidence).unwrap();
    let restored: SfdEvidenceV1 = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored, evidence);
}
