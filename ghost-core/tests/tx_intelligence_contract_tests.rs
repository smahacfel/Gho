use ghost_core::tx_intelligence::types::TxIntelFeatures;

#[test]
fn tx_intelligence_top3_signer_volume_ratio_new_payload_uses_option_field() {
    let features = TxIntelFeatures {
        tx_count: 4,
        total_volume_sol: 10.0,
        top3_signer_volume_ratio: Some(0.60),
        top3_volume_pct: 0.60,
        ..TxIntelFeatures::default()
    };

    assert_eq!(features.top3_signer_volume_ratio, Some(0.60));
    assert!((features.effective_top3_signer_volume_ratio() - 0.60).abs() < f64::EPSILON);

    let encoded = serde_json::to_value(&features).expect("tx intel should serialize");
    assert_eq!(encoded["top3_signer_volume_ratio"], serde_json::json!(0.60));
    assert_eq!(encoded["top3_volume_pct"], serde_json::json!(0.60));
}

#[test]
fn tx_intelligence_top3_signer_volume_ratio_legacy_payload_falls_back_without_silent_zero() {
    let legacy_features = TxIntelFeatures {
        tx_count: 4,
        total_volume_sol: 10.0,
        top3_signer_volume_ratio: Some(0.60),
        top3_volume_pct: 0.73,
        ..TxIntelFeatures::default()
    };
    let mut legacy_payload =
        serde_json::to_value(&legacy_features).expect("tx intel should serialize");
    legacy_payload
        .as_object_mut()
        .expect("tx intel payload should be an object")
        .remove("top3_signer_volume_ratio");

    let decoded: TxIntelFeatures =
        serde_json::from_value(legacy_payload).expect("legacy tx intel should deserialize");

    assert_eq!(decoded.top3_signer_volume_ratio, None);
    assert!((decoded.top3_volume_pct - 0.73).abs() < f64::EPSILON);
    assert!((decoded.effective_top3_signer_volume_ratio() - 0.73).abs() < f64::EPSILON);
}
