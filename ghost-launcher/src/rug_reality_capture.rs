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

use crate::rug_scalp_v2::RugScalpRuntimeFeeAuthorityManifestV1;

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
    pub code_hash: String,
    pub cost_profile: RugRealityCostProfileV1,
}

impl Default for RugRealityCaptureConfigV1 {
    fn default() -> Self {
        Self {
            enabled: false,
            run_id: String::new(),
            manifest_path: String::new(),
            code_hash: String::new(),
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
            ("rug_reality_capture.code_hash", self.code_hash.as_str()),
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
}

/// Immutable run-level authority and cost receipt written once at capture
/// startup. It is intentionally not emitted per trade and cannot substitute
/// fixture evidence for the runtime fee registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RugRealityCaptureRunManifestV1 {
    pub schema_version: u16,
    pub run_id: String,
    pub observe_only: bool,
    pub signal_detector: String,
    pub entry_route_id: String,
    pub exit_route_id: String,
    pub config_hash: String,
    pub code_hash: String,
    pub binary_hash: String,
    pub cost_profile: RugRealityCostProfileV1,
    pub runtime_fee_authority: RugScalpRuntimeFeeAuthorityManifestV1,
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
        };
        assert!(config.validate_enabled_contract().is_err());
    }
}
