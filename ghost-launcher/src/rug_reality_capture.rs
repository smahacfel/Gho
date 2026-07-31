//! Minimal configuration and immutable receipt for the full-universe RUG
//! reality capture.  This module owns no signal, position, quote, or runtime
//! lifecycle: it only freezes the evidence contract used to interpret the
//! durable Pump transaction tape offline.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use anyhow::{bail, Context, Result};
use ghost_core::TransactionCosts;
use serde::{Deserialize, Serialize};

use crate::rug_scalp_v2::{RugScalpPumpQuoteAuthorityV1, RugScalpRuntimeFeeAuthorityManifestV1};

/// Frozen transaction-envelope policy. Pump program settlement remains owned
/// by the independently materialized on-chain fee schedules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RugRealityCostProfileV1 {
    pub profile_id: String,
    pub entry_transaction_costs: TransactionCosts,
    pub exit_transaction_costs: TransactionCosts,
    pub entry_compute_unit_limit: u32,
    pub entry_compute_unit_price_micro_lamports: u64,
    pub exit_compute_unit_limit: u32,
    pub exit_compute_unit_price_micro_lamports: u64,
    pub tip_policy_id: String,
    pub ata_policy_id: String,
    pub retry_policy_id: String,
    pub quote_age_policy_ms: u64,
}

impl Default for RugRealityCostProfileV1 {
    fn default() -> Self {
        Self {
            profile_id: String::new(),
            entry_transaction_costs: TransactionCosts::default(),
            exit_transaction_costs: TransactionCosts::default(),
            entry_compute_unit_limit: 0,
            entry_compute_unit_price_micro_lamports: 0,
            exit_compute_unit_limit: 0,
            exit_compute_unit_price_micro_lamports: 0,
            tip_policy_id: String::new(),
            ata_policy_id: String::new(),
            retry_policy_id: String::new(),
            quote_age_policy_ms: 0,
        }
    }
}

/// Observe-only full-universe capture.  Keeping this separate from the
/// rejected V2 detector ensures it cannot instantiate a reducer, validation
/// tape, Position Manager lifecycle, or entry/exit intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RugRealityCaptureConfigV1 {
    pub enabled: bool,
    pub run_id: String,
    pub manifest_path: String,
    /// Scientific parent against which the experiment is framed.  This is not
    /// the source revision of the capture implementation.
    pub baseline_sha: String,
    /// Immutable Git revision containing the ACE capture/probe implementation.
    /// The runtime refuses a capture when it is not paired with `code_hash`.
    pub implementation_sha: String,
    pub code_hash: String,
    /// Manifest-adjacent, post-shutdown health evidence consumed by the
    /// offline probe.  The manifest remains immutable; the evidence is written
    /// once only after the operator has collected the final metrics snapshot.
    pub health_evidence_path: String,
    pub cost_profile: RugRealityCostProfileV1,
}

impl Default for RugRealityCaptureConfigV1 {
    fn default() -> Self {
        Self {
            enabled: false,
            run_id: String::new(),
            manifest_path: String::new(),
            baseline_sha: String::new(),
            implementation_sha: String::new(),
            code_hash: String::new(),
            health_evidence_path: String::new(),
            cost_profile: RugRealityCostProfileV1::default(),
        }
    }
}

impl RugRealityCaptureConfigV1 {
    pub fn validate_enabled_contract(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        for (name, value) in [
            ("rug_reality_capture.run_id", self.run_id.as_str()),
            (
                "rug_reality_capture.manifest_path",
                self.manifest_path.as_str(),
            ),
            (
                "rug_reality_capture.baseline_sha",
                self.baseline_sha.as_str(),
            ),
            (
                "rug_reality_capture.implementation_sha",
                self.implementation_sha.as_str(),
            ),
            ("rug_reality_capture.code_hash", self.code_hash.as_str()),
            (
                "rug_reality_capture.health_evidence_path",
                self.health_evidence_path.as_str(),
            ),
            (
                "rug_reality_capture.cost_profile.profile_id",
                self.cost_profile.profile_id.as_str(),
            ),
            (
                "rug_reality_capture.cost_profile.tip_policy_id",
                self.cost_profile.tip_policy_id.as_str(),
            ),
            (
                "rug_reality_capture.cost_profile.ata_policy_id",
                self.cost_profile.ata_policy_id.as_str(),
            ),
            (
                "rug_reality_capture.cost_profile.retry_policy_id",
                self.cost_profile.retry_policy_id.as_str(),
            ),
        ] {
            if value.trim().is_empty() {
                bail!("{name} is required when full-universe capture is enabled");
            }
        }
        if !is_git_sha(&self.baseline_sha) {
            bail!("rug_reality_capture.baseline_sha must be a full 40-character Git SHA");
        }
        if !is_git_sha(&self.implementation_sha) {
            bail!("rug_reality_capture.implementation_sha must be a full 40-character Git SHA");
        }
        if self.code_hash != format!("git:{}", self.implementation_sha) {
            bail!(
                "rug_reality_capture.code_hash must equal git:<rug_reality_capture.implementation_sha>"
            );
        }
        if self.cost_profile.entry_compute_unit_limit == 0
            || self.cost_profile.exit_compute_unit_limit == 0
        {
            bail!("rug_reality_capture requires non-zero entry and exit CU limits");
        }
        if self.cost_profile.quote_age_policy_ms == 0 {
            bail!("rug_reality_capture.cost_profile.quote_age_policy_ms must be non-zero");
        }
        if self.cost_profile.entry_transaction_costs.base_fee_lamports == 0
            || self.cost_profile.exit_transaction_costs.base_fee_lamports == 0
        {
            bail!("rug_reality_capture requires frozen non-zero entry and exit base fees");
        }
        Ok(())
    }

    /// PoolTransaction is an optional EventWriter event. A full-universe
    /// capture without that tape cannot support the offline ACE probe, so the
    /// capture must fail before any runtime component is constructed.
    pub fn validate_event_writer_contract(&self, enable_optional_events: bool) -> Result<()> {
        if self.enabled && !enable_optional_events {
            bail!(
                "rug_reality_capture.enabled requires execution.events.enable_optional_events=true because PoolTransaction evidence is optional"
            );
        }
        Ok(())
    }
}

fn is_git_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Immutable run-level authority and cost receipt written once at capture
/// startup. It is intentionally not emitted per trade and cannot substitute
/// fixture evidence for the runtime fee registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RugRealityCaptureRunManifestV1 {
    pub schema_version: u16,
    pub run_id: String,
    pub observe_only: bool,
    pub signal_detector: String,
    pub entry_route_id: String,
    pub exit_route_id: String,
    pub config_hash: String,
    /// Frozen scientific parent (PR86), distinct from the implementation
    /// source revision and from the hash of the executable binary.
    #[serde(default)]
    pub baseline_sha: String,
    /// Frozen source revision of the ACE capture/probe implementation.
    #[serde(default)]
    pub implementation_sha: String,
    pub code_hash: String,
    pub binary_hash: String,
    /// Immutable location reserved at startup for the single post-shutdown
    /// health-evidence artifact.  It is not a registry or a runtime service.
    #[serde(default)]
    pub health_evidence_path: String,
    /// PR1E canonical-runtime authority epoch active for this one capture.
    /// A zero value decodes old manifests but is not valid input to the ACE
    /// probe, which requires one explicit authority epoch.
    #[serde(default)]
    pub authority_epoch_id: u64,
    /// Capture-time EventWriter run ID. It is duplicated deliberately so an
    /// offline consumer can reject tape from a different writer run.
    #[serde(default)]
    pub event_writer_run_id: String,
    /// `PoolTransaction` is optional in EventWriter. Persisting this bit makes
    /// a disabled optional-event surface an explicit invalid-capture fact,
    /// rather than trying to infer it from a missing tape after the fact.
    #[serde(default)]
    pub event_writer_optional_events_enabled: bool,
    pub cost_profile: RugRealityCostProfileV1,
    pub runtime_fee_authority: RugScalpRuntimeFeeAuthorityManifestV1,
    /// Serialized typed Pump quote authority frozen before capture. The
    /// offline probe materializes this exact authority and never performs a
    /// later RPC lookup for historical slots.
    #[serde(default)]
    pub pump_quote_authority: RugScalpPumpQuoteAuthorityV1,
}

/// One durable, manifest-bound result of the capture health check.
///
/// This is deliberately an offline evidence artifact: the launcher does not
/// mutate it and the probe never opens a metrics endpoint.  The operator first
/// captures manifest-bound start/end loopback snapshots, then this receipt is
/// materialized only after a controlled shutdown and consumed fail-closed by
/// the probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RugRealityCaptureHealthEvidenceV1 {
    pub schema_version: u16,
    pub run_id: String,
    pub manifest_sha256: String,
    /// `smoke` or `day1`; the probe validates the corresponding duration
    /// contract rather than trusting a receipt from an arbitrary time span.
    pub capture_kind: String,
    /// SHA-256 of the structured start snapshot, which itself binds the run
    /// and manifest before exposing its raw metrics digest.
    pub start_snapshot_sha256: String,
    /// SHA-256 of the structured end snapshot.
    pub end_snapshot_sha256: String,
    /// SHA-256 of the raw metrics body captured by the start snapshot.
    pub start_metrics_sha256: String,
    /// SHA-256 of the raw metrics body captured by the end snapshot.
    pub end_metrics_sha256: String,
    pub start_captured_at_unix_ms: u64,
    pub end_captured_at_unix_ms: u64,
    pub duration_ms: u64,
    pub pr1_runtime_bypass_attempt_total: u64,
    pub pr1_runtime_candidate_admission_closed_total: u64,
    pub pr1_runtime_primary_coverage_gap_total: u64,
    /// Non-zero when the launcher deliberately continued after an interval
    /// whose canonical completeness could not be proved. Such continuation is
    /// forensic only; the offline probe rejects the capture.
    #[serde(default)]
    pub ace_capture_segment_invalid_total: u64,
    pub event_writer_write_failure_count: u64,
    pub event_writer_lock_failure_count: u64,
    pub controlled_shutdown: bool,
    pub event_files_cleanly_flushed: bool,
    pub log_evidence_clean: bool,
}

pub fn write_rug_reality_capture_run_manifest_new(
    path: &Path,
    manifest: &RugRealityCaptureRunManifestV1,
) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create RUG reality manifest directory {}", parent.display()))?;
    let bytes = serde_json::to_vec_pretty(manifest)
        .context("serialize RUG reality capture run manifest")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| {
            format!(
                "create immutable RUG reality capture run manifest {}",
                path.display()
            )
        })?;
    file.write_all(&bytes)
        .context("write RUG reality capture run manifest")?;
    file.write_all(b"\n")
        .context("terminate RUG reality capture run manifest")?;
    file.sync_all()
        .context("sync RUG reality capture run manifest")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_capture_requires_no_runtime_surface() {
        assert!(RugRealityCaptureConfigV1::default()
            .validate_enabled_contract()
            .is_ok());
    }

    #[test]
    fn enabled_capture_rejects_missing_frozen_cost_evidence() {
        let config = RugRealityCaptureConfigV1 {
            enabled: true,
            run_id: "reality-r1".to_string(),
            manifest_path: "manifest.json".to_string(),
            code_hash: "code".to_string(),
            cost_profile: RugRealityCostProfileV1 {
                profile_id: "current-shadow".to_string(),
                entry_compute_unit_limit: 400_000,
                exit_compute_unit_limit: 400_000,
                quote_age_policy_ms: 1_500,
                tip_policy_id: "tip".to_string(),
                ata_policy_id: "ata".to_string(),
                retry_policy_id: "retry".to_string(),
                ..RugRealityCostProfileV1::default()
            },
            ..RugRealityCaptureConfigV1::default()
        };
        assert!(config.validate_enabled_contract().is_err());
    }

    #[test]
    fn enabled_capture_requires_optional_transaction_events() {
        let config = RugRealityCaptureConfigV1 {
            enabled: true,
            ..RugRealityCaptureConfigV1::default()
        };
        assert!(config.validate_event_writer_contract(false).is_err());
        assert!(config.validate_event_writer_contract(true).is_ok());
    }
}
