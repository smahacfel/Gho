use serde::{Deserialize, Serialize};
use serde_json::json;

pub const DEFAULT_V3_POLICY_VERSION: u32 = 1;
pub const DEFAULT_V3_MATERIALIZATION_VERSION: u32 = 1;

fn default_v3_policy_version() -> u32 {
    DEFAULT_V3_POLICY_VERSION
}

fn default_v3_materialization_version() -> u32 {
    DEFAULT_V3_MATERIALIZATION_VERSION
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatekeeperV3Config {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub shadow_emit_enabled: bool,
    #[serde(default = "default_v3_policy_version")]
    pub policy_version: u32,
    #[serde(default = "default_v3_materialization_version")]
    pub materialization_version: u32,
    #[serde(default)]
    pub promotion: GatekeeperV3PromotionConfig,
    #[serde(default)]
    pub thresholds: GatekeeperV3Thresholds,
}

impl Default for GatekeeperV3Config {
    fn default() -> Self {
        Self {
            enabled: false,
            shadow_emit_enabled: false,
            policy_version: DEFAULT_V3_POLICY_VERSION,
            materialization_version: DEFAULT_V3_MATERIALIZATION_VERSION,
            promotion: GatekeeperV3PromotionConfig::default(),
            thresholds: GatekeeperV3Thresholds::default(),
        }
    }
}

impl GatekeeperV3Config {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.policy_version == 0 {
            anyhow::bail!("gatekeeper_v3.policy_version must be > 0");
        }
        if self.materialization_version == 0 {
            anyhow::bail!("gatekeeper_v3.materialization_version must be > 0");
        }
        self.thresholds.validate()
    }

    pub fn v3_policy_config_hash(&self) -> String {
        let payload = self.canonical_policy_payload();
        // `serde_json::Value` serialization cannot fail for this in-memory
        // payload; this panic would indicate a serde_json invariant violation.
        let bytes = serde_json::to_vec(&payload).expect("canonical V3 policy payload serializes");
        blake3::hash(&bytes).to_hex().to_string()
    }

    pub fn stage_thresholds_payload(&self) -> serde_json::Value {
        let t = &self.thresholds;
        json!({
            "evidence": {
                "materialization_version": self.materialization_version,
                "min_tx_count": t.min_tx_count,
                "min_unique_signers": t.min_unique_signers,
                "min_buy_count": t.min_buy_count
            },
            "risk": {
                "reject_on_dev_sell": t.reject_on_dev_sell,
                "hard_fail_hhi": t.hard_fail_hhi,
                "hard_fail_same_ms_tx_ratio": t.hard_fail_same_ms_tx_ratio,
                "hard_fail_top3_volume_pct": t.hard_fail_top3_volume_pct,
                "max_tx_per_signer": t.max_tx_per_signer,
                "max_dev_volume_ratio": t.max_dev_volume_ratio,
                "max_signer_cross_pool_velocity": t.max_signer_cross_pool_velocity,
                "max_funding_source_concentration": t.max_funding_source_concentration
            },
            "opportunity": {
                "min_buy_ratio": t.min_buy_ratio,
                "max_buy_ratio": t.max_buy_ratio,
                "max_hhi": t.max_hhi,
                "organic_min_tx_count_growth_ratio": t.organic_min_tx_count_growth_ratio,
                "organic_min_unique_signer_growth_ratio": t.organic_min_unique_signer_growth_ratio
            },
            "confidence": {
                "execution_not_run_confidence_cap": t.execution_not_run_confidence_cap
            }
        })
    }

    fn canonical_policy_payload(&self) -> serde_json::Value {
        let t = &self.thresholds;
        json!({
            "enabled": self.enabled,
            "shadow_emit_enabled": self.shadow_emit_enabled,
            "policy_version": self.policy_version,
            "materialization_version": self.materialization_version,
            "promotion": {
                "enabled": self.promotion.enabled
            },
            "thresholds": {
                "execution_not_run_confidence_cap_bits": f64_bits(t.execution_not_run_confidence_cap),
                "hard_fail_hhi_bits": f64_bits(t.hard_fail_hhi),
                "hard_fail_same_ms_tx_ratio_bits": f64_bits(t.hard_fail_same_ms_tx_ratio),
                "hard_fail_top3_volume_pct_bits": f64_bits(t.hard_fail_top3_volume_pct),
                "max_buy_ratio_bits": f64_bits(t.max_buy_ratio),
                "max_dev_volume_ratio_bits": f64_bits(t.max_dev_volume_ratio),
                "max_funding_source_concentration_bits": f64_bits(t.max_funding_source_concentration),
                "max_hhi_bits": f64_bits(t.max_hhi),
                "max_signer_cross_pool_velocity_bits": f64_bits(t.max_signer_cross_pool_velocity),
                "max_tx_per_signer": t.max_tx_per_signer,
                "min_buy_count": t.min_buy_count,
                "min_buy_ratio_bits": f64_bits(t.min_buy_ratio),
                "min_tx_count": t.min_tx_count,
                "min_unique_signers": t.min_unique_signers,
                "organic_min_tx_count_growth_ratio_bits": f64_bits(t.organic_min_tx_count_growth_ratio),
                "organic_min_unique_signer_growth_ratio_bits": f64_bits(t.organic_min_unique_signer_growth_ratio),
                "reject_on_dev_sell": t.reject_on_dev_sell
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GatekeeperV3PromotionConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatekeeperV3Thresholds {
    #[serde(default = "default_min_tx_count")]
    pub min_tx_count: u64,
    #[serde(default = "default_min_unique_signers")]
    pub min_unique_signers: u64,
    #[serde(default = "default_min_buy_count")]
    pub min_buy_count: u64,
    #[serde(default = "default_min_buy_ratio")]
    pub min_buy_ratio: f64,
    #[serde(default = "default_max_buy_ratio")]
    pub max_buy_ratio: f64,
    #[serde(default = "default_max_hhi")]
    pub max_hhi: f64,
    #[serde(default = "default_hard_fail_hhi")]
    pub hard_fail_hhi: f64,
    #[serde(default = "default_hard_fail_same_ms_tx_ratio")]
    pub hard_fail_same_ms_tx_ratio: f64,
    #[serde(default = "default_hard_fail_top3_volume_pct")]
    pub hard_fail_top3_volume_pct: f64,
    #[serde(default = "default_max_tx_per_signer")]
    pub max_tx_per_signer: u64,
    #[serde(default = "default_max_dev_volume_ratio")]
    pub max_dev_volume_ratio: f64,
    #[serde(default)]
    pub reject_on_dev_sell: bool,
    #[serde(default = "default_max_signer_cross_pool_velocity")]
    pub max_signer_cross_pool_velocity: f64,
    #[serde(default = "default_max_funding_source_concentration")]
    pub max_funding_source_concentration: f64,
    #[serde(default = "default_organic_min_growth_ratio")]
    pub organic_min_tx_count_growth_ratio: f64,
    #[serde(default = "default_organic_min_growth_ratio")]
    pub organic_min_unique_signer_growth_ratio: f64,
    #[serde(default = "default_execution_not_run_confidence_cap")]
    pub execution_not_run_confidence_cap: f64,
}

impl Default for GatekeeperV3Thresholds {
    fn default() -> Self {
        Self {
            min_tx_count: default_min_tx_count(),
            min_unique_signers: default_min_unique_signers(),
            min_buy_count: default_min_buy_count(),
            min_buy_ratio: default_min_buy_ratio(),
            max_buy_ratio: default_max_buy_ratio(),
            max_hhi: default_max_hhi(),
            hard_fail_hhi: default_hard_fail_hhi(),
            hard_fail_same_ms_tx_ratio: default_hard_fail_same_ms_tx_ratio(),
            hard_fail_top3_volume_pct: default_hard_fail_top3_volume_pct(),
            max_tx_per_signer: default_max_tx_per_signer(),
            max_dev_volume_ratio: default_max_dev_volume_ratio(),
            reject_on_dev_sell: false,
            max_signer_cross_pool_velocity: default_max_signer_cross_pool_velocity(),
            max_funding_source_concentration: default_max_funding_source_concentration(),
            organic_min_tx_count_growth_ratio: default_organic_min_growth_ratio(),
            organic_min_unique_signer_growth_ratio: default_organic_min_growth_ratio(),
            execution_not_run_confidence_cap: default_execution_not_run_confidence_cap(),
        }
    }
}

impl GatekeeperV3Thresholds {
    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, value) in [
            ("min_buy_ratio", self.min_buy_ratio),
            ("max_buy_ratio", self.max_buy_ratio),
            ("max_hhi", self.max_hhi),
            ("hard_fail_hhi", self.hard_fail_hhi),
            ("hard_fail_same_ms_tx_ratio", self.hard_fail_same_ms_tx_ratio),
            ("hard_fail_top3_volume_pct", self.hard_fail_top3_volume_pct),
            ("max_dev_volume_ratio", self.max_dev_volume_ratio),
            (
                "max_signer_cross_pool_velocity",
                self.max_signer_cross_pool_velocity,
            ),
            (
                "max_funding_source_concentration",
                self.max_funding_source_concentration,
            ),
            (
                "organic_min_tx_count_growth_ratio",
                self.organic_min_tx_count_growth_ratio,
            ),
            (
                "organic_min_unique_signer_growth_ratio",
                self.organic_min_unique_signer_growth_ratio,
            ),
            (
                "execution_not_run_confidence_cap",
                self.execution_not_run_confidence_cap,
            ),
        ] {
            if !value.is_finite() {
                anyhow::bail!("gatekeeper_v3.thresholds.{name} must be finite");
            }
            if value < 0.0 {
                anyhow::bail!("gatekeeper_v3.thresholds.{name} must be non-negative");
            }
        }
        if self.min_buy_ratio > self.max_buy_ratio {
            anyhow::bail!(
                "gatekeeper_v3.thresholds.min_buy_ratio must be <= max_buy_ratio"
            );
        }
        if self.execution_not_run_confidence_cap > 1.0 {
            anyhow::bail!(
                "gatekeeper_v3.thresholds.execution_not_run_confidence_cap must be <= 1.0"
            );
        }
        Ok(())
    }
}

fn f64_bits(value: f64) -> String {
    let bits = if value == 0.0 { 0 } else { value.to_bits() };
    format!("{bits:016x}")
}

const fn default_min_tx_count() -> u64 {
    30
}

const fn default_min_unique_signers() -> u64 {
    15
}

const fn default_min_buy_count() -> u64 {
    15
}

const fn default_min_buy_ratio() -> f64 {
    0.50
}

const fn default_max_buy_ratio() -> f64 {
    1.0
}

const fn default_max_hhi() -> f64 {
    0.25
}

const fn default_hard_fail_hhi() -> f64 {
    0.10
}

const fn default_hard_fail_same_ms_tx_ratio() -> f64 {
    0.60
}

const fn default_hard_fail_top3_volume_pct() -> f64 {
    0.70
}

const fn default_max_tx_per_signer() -> u64 {
    4
}

const fn default_max_dev_volume_ratio() -> f64 {
    0.40
}

const fn default_max_signer_cross_pool_velocity() -> f64 {
    1.0
}

const fn default_max_funding_source_concentration() -> f64 {
    1.0
}

const fn default_organic_min_growth_ratio() -> f64 {
    1.0
}

const fn default_execution_not_run_confidence_cap() -> f64 {
    0.80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gatekeeper_v3_config_defaults_are_shadow_disabled() {
        let config = GatekeeperV3Config::default();
        assert!(!config.enabled);
        assert!(!config.shadow_emit_enabled);
        assert_eq!(config.policy_version, 1);
        assert_eq!(config.materialization_version, 1);
        assert!(!config.promotion.enabled);
    }

    #[test]
    fn gatekeeper_v3_config_hash_is_stable_and_sensitive() {
        let config = GatekeeperV3Config::default();
        assert_eq!(config.v3_policy_config_hash(), config.v3_policy_config_hash());

        let mut changed = config.clone();
        changed.thresholds.min_tx_count += 1;
        assert_ne!(config.v3_policy_config_hash(), changed.v3_policy_config_hash());
    }

    #[test]
    fn gatekeeper_v3_config_rejects_invalid_thresholds() {
        let mut config = GatekeeperV3Config::default();
        config.thresholds.execution_not_run_confidence_cap = 1.1;
        assert!(config.validate().is_err());
    }
}
