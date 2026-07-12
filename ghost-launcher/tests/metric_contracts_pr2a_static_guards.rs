use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn read(path: &str) -> String {
    fs::read_to_string(repo_root().join(path))
        .unwrap_or_else(|error| panic!("failed to read static-guard path {path}: {error}"))
}

#[test]
fn pr2a_does_not_activate_counterfactual_or_evidence_only_policy_inputs() {
    let policy = read("ghost-launcher/src/components/gatekeeper_policy.rs");
    for forbidden in [
        "FtdiUniqueBuyerActionabilityV2",
        "MfsDevPrimaryBuySolV1",
        "FundingSourceV2ReadinessEvidence",
        "RceSameMsCollisionRatioRecentExact",
        "same_ms_tx_ratio_recent",
        "metric_contract_decision_projection_v1",
    ] {
        assert!(
            !policy.contains(forbidden),
            "active Gatekeeper policy references PR2A non-authoritative surface: {forbidden}"
        );
    }
}

#[test]
fn pr2a_leaves_v33_logger_and_v3_replay_without_projection_activation() {
    let logger = read("ghost-brain/src/oracle/decision_logger.rs");
    let replay = read("ghost-launcher/src/bin/v3_replay.rs");
    for (name, source) in [("decision_logger", logger), ("v3_replay", replay)] {
        assert!(
            !source.contains("MetricContractDecisionEvidenceProjectionV1"),
            "{name} must not activate PR2A projection"
        );
        assert!(
            !source.contains("METRIC_CONTRACT_DECISION_SCHEMA_VERSION_V34"),
            "{name} must remain on its frozen schema in PR2A"
        );
    }
}

#[test]
fn pr2a_does_not_materialize_partial_root_or_pr2b_family_builders() {
    let mfs = read("ghost-core/src/checkpoint/types.rs");
    assert!(!mfs.contains("metric_contract_decision_projection_v1"));
    let projection = read("ghost-core/src/metric_contracts/projection.rs");
    for forbidden in [
        "impl FlipDecisionProjectionV1",
        "impl ManipulationDecisionProjectionV1",
        "impl ReserveVelocityDecisionProjectionV1",
        "impl RecentBuySellDecisionProjectionV1",
    ] {
        assert!(
            !projection.contains(forbidden),
            "PR2B family builder activated in PR2A: {forbidden}"
        );
    }
}

#[test]
fn projection_builder_is_pure_and_runtime_stays_legacy_only() {
    let projection = read("ghost-core/src/metric_contracts/projection.rs");
    for forbidden in [
        "PoolTransaction",
        "GatekeeperBuffer",
        "FundingSourceIndex::",
        "use crate::tx_intelligence::FundingSourceIndex",
        "materialize_features",
        "SystemTime",
        "Instant",
    ] {
        assert!(
            !projection.contains(forbidden),
            "projection builder reads producer/live state: {forbidden}"
        );
    }
    assert!(projection.contains("pub fn validated_canonical_hash("));
    assert!(!projection.contains("pub fn canonical_hash(&self)"));
    assert!(!projection.contains("TX_TIMING_RECENT_EXACT_WINDOW_MS_V1"));
    assert!(!projection.contains("FSC_LEGACY_MIN_KNOWN_SOURCE_SAMPLES_V1"));
    assert!(projection.contains("SameMsRecentWindowMs"));
    assert!(projection.contains("FscLegacyMinKnownSourceSamples"));
    let pr2a = read("ghost-launcher/src/metric_contracts/pr2a.rs");
    assert!(pr2a.contains(".validated_canonical_hash(context)"));
    let main = read("ghost-launcher/src/main.rs");
    assert!(main
        .contains("foundation.metric_contract_rollout_mode != MetricContractRolloutMode::Legacy"));
    assert!(!main.contains("activation = \"dual_compute\""));
    assert!(!main.contains("activation = \"v2\""));
}

#[test]
fn active_tx_intel_top3_reads_stay_behind_the_single_effective_selector() {
    let policy = read("ghost-launcher/src/components/gatekeeper_policy.rs");
    let production_policy = policy.split("#[cfg(test)]").next().unwrap_or(&policy);
    assert!(
        !production_policy.contains("tx_intel_features.top3_volume_pct"),
        "active policy must not read the legacy top3 alias directly"
    );
    assert!(production_policy.contains("effective_top3_signer_volume_ratio()"));

    let engine = read("ghost-launcher/src/tx_intelligence/engine.rs");
    assert!(engine.contains("features.effective_top3_signer_volume_ratio()"));
    assert_eq!(
        read("ghost-core/src/tx_intelligence/types.rs")
            .matches("fn effective_top3_signer_volume_ratio")
            .count(),
        1,
        "TxIntelFeatures must retain exactly one canonical selector helper"
    );
}

#[test]
fn pr2a_keeps_one_canonical_producer_per_parity_sensitive_metric() {
    assert_eq!(
        read("ghost-launcher/src/tx_intelligence/sybil_metrics.rs")
            .matches("fn compute_ftdi_from_buys")
            .count(),
        1
    );
    assert_eq!(
        read("ghost-launcher/src/tx_intelligence/funding_source.rs")
            .matches("1.0 - (distinct_known_sources as f64 / known_sources.len() as f64)")
            .count(),
        1
    );
    assert_eq!(
        read("ghost-launcher/src/tx_intelligence/engine.rs")
            .matches("fn dev_primary_snapshot")
            .count(),
        1
    );
}

#[test]
fn type5_runtime_is_not_part_of_pr2a() {
    for path in ["ghost-core/src", "ghost-launcher/src", "ghost-brain/src"] {
        let output = std::process::Command::new("rg")
            .args([
                "-l",
                "EarlyFlowPatternAssessmentV1|CoordinationPatternAssessmentV1",
                path,
            ])
            .current_dir(repo_root())
            .output()
            .expect("run rg Type-5 guard");
        assert!(
            !output.status.success() && output.stdout.is_empty(),
            "Type-5 runtime symbol appeared under {path}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
