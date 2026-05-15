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

fn default_early_window_ms() -> u64 {
    2_000
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatekeeperV3Config {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub shadow_emit_enabled: bool,
    #[serde(default)]
    pub replay_payload_enabled: bool,
    #[serde(default = "default_v3_policy_version")]
    pub policy_version: u32,
    #[serde(default = "default_v3_materialization_version")]
    pub materialization_version: u32,
    #[serde(default = "default_early_window_ms")]
    pub early_window_ms: u64,
    #[serde(default)]
    pub promotion: GatekeeperV3PromotionConfig,
    #[serde(default)]
    pub early: GatekeeperV3StageProfile,
    #[serde(default)]
    pub normal: GatekeeperV3StageProfile,
    #[serde(default)]
    pub extended: GatekeeperV3StageProfile,
    #[serde(default)]
    pub evidence_requirements: GatekeeperV3EvidenceRequirements,
    #[serde(default)]
    pub confidence_caps: GatekeeperV3ConfidenceCaps,
    #[serde(default)]
    pub component_weights: GatekeeperV3ComponentWeights,
}

impl Default for GatekeeperV3Config {
    fn default() -> Self {
        Self {
            enabled: false,
            shadow_emit_enabled: false,
            replay_payload_enabled: false,
            policy_version: DEFAULT_V3_POLICY_VERSION,
            materialization_version: DEFAULT_V3_MATERIALIZATION_VERSION,
            early_window_ms: default_early_window_ms(),
            promotion: GatekeeperV3PromotionConfig::default(),
            early: GatekeeperV3StageProfile::default(),
            normal: GatekeeperV3StageProfile::default(),
            extended: GatekeeperV3StageProfile::default(),
            evidence_requirements: GatekeeperV3EvidenceRequirements::default(),
            confidence_caps: GatekeeperV3ConfidenceCaps::default(),
            component_weights: GatekeeperV3ComponentWeights::default(),
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
        self.early.validate("early")?;
        self.normal.validate("normal")?;
        self.extended.validate("extended")?;
        self.confidence_caps.validate()?;
        self.component_weights.validate()?;
        Ok(())
    }

    pub fn v3_policy_config_hash(&self) -> String {
        let payload = self.canonical_policy_payload();
        // `serde_json::Value` serialization cannot fail for this in-memory
        // payload; this panic would indicate a serde_json invariant violation.
        let bytes = serde_json::to_vec(&payload).expect("canonical V3 policy payload serializes");
        blake3::hash(&bytes).to_hex().to_string()
    }

    pub fn stage_thresholds_payload(&self) -> serde_json::Value {
        json!({
            "selector": {
                "early_window_ms": self.early_window_ms
            },
            "profiles": {
                "early": self.early.payload(),
                "normal": self.normal.payload(),
                "extended": self.extended.payload()
            },
            "evidence": {
                "materialization_version": self.materialization_version,
                "requirements": self.evidence_requirements.payload()
            },
            "confidence": self.confidence_caps.payload(),
            "component_weights": self.component_weights.payload()
        })
    }

    fn canonical_policy_payload(&self) -> serde_json::Value {
        json!({
            "enabled": self.enabled,
            "shadow_emit_enabled": self.shadow_emit_enabled,
            "policy_version": self.policy_version,
            "materialization_version": self.materialization_version,
            "early_window_ms": self.early_window_ms,
            "promotion": {
                "enabled": self.promotion.enabled
            },
            "profiles": {
                "early": self.early.canonical_payload(),
                "normal": self.normal.canonical_payload(),
                "extended": self.extended.canonical_payload()
            },
            "evidence_requirements": self.evidence_requirements.canonical_payload(),
            "confidence_caps": self.confidence_caps.canonical_payload(),
            "component_weights": self.component_weights.canonical_payload()
        })
    }

    pub fn profile_for_deadline(&self, deadline_elapsed: bool) -> &GatekeeperV3StageProfile {
        self.profile_for_context(deadline_elapsed, self.early_window_ms)
    }

    pub fn profile_for_context(
        &self,
        deadline_elapsed: bool,
        observation_duration_ms: u64,
    ) -> &GatekeeperV3StageProfile {
        match self.profile_name_for_context(deadline_elapsed, observation_duration_ms) {
            "extended" => &self.extended,
            "early" => &self.early,
            _ => &self.normal,
        }
    }

    pub fn profile_name_for_context(
        &self,
        deadline_elapsed: bool,
        observation_duration_ms: u64,
    ) -> &'static str {
        if deadline_elapsed {
            "extended"
        } else if observation_duration_ms < self.early_window_ms {
            "early"
        } else {
            "normal"
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GatekeeperV3PromotionConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatekeeperV3StageProfile {
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
}

impl Default for GatekeeperV3StageProfile {
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
        }
    }
}

impl GatekeeperV3StageProfile {
    pub fn validate(&self, profile_name: &str) -> anyhow::Result<()> {
        for (name, value) in [
            ("min_buy_ratio", self.min_buy_ratio),
            ("max_buy_ratio", self.max_buy_ratio),
            ("max_hhi", self.max_hhi),
            ("hard_fail_hhi", self.hard_fail_hhi),
            (
                "hard_fail_same_ms_tx_ratio",
                self.hard_fail_same_ms_tx_ratio,
            ),
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
        ] {
            if !value.is_finite() {
                anyhow::bail!("gatekeeper_v3.{profile_name}.{name} must be finite");
            }
            if value < 0.0 {
                anyhow::bail!("gatekeeper_v3.{profile_name}.{name} must be non-negative");
            }
        }
        if self.min_buy_ratio > self.max_buy_ratio {
            anyhow::bail!("gatekeeper_v3.{profile_name}.min_buy_ratio must be <= max_buy_ratio");
        }
        Ok(())
    }

    fn payload(&self) -> serde_json::Value {
        json!({
            "evidence": {
                "min_tx_count": self.min_tx_count,
                "min_unique_signers": self.min_unique_signers,
                "min_buy_count": self.min_buy_count
            },
            "risk": {
                "reject_on_dev_sell": self.reject_on_dev_sell,
                "hard_fail_hhi": self.hard_fail_hhi,
                "hard_fail_same_ms_tx_ratio": self.hard_fail_same_ms_tx_ratio,
                "hard_fail_top3_volume_pct": self.hard_fail_top3_volume_pct,
                "max_tx_per_signer": self.max_tx_per_signer,
                "max_dev_volume_ratio": self.max_dev_volume_ratio,
                "max_signer_cross_pool_velocity": self.max_signer_cross_pool_velocity,
                "max_funding_source_concentration": self.max_funding_source_concentration
            },
            "opportunity": {
                "min_buy_ratio": self.min_buy_ratio,
                "max_buy_ratio": self.max_buy_ratio,
                "max_hhi": self.max_hhi,
                "organic_min_tx_count_growth_ratio": self.organic_min_tx_count_growth_ratio,
                "organic_min_unique_signer_growth_ratio": self.organic_min_unique_signer_growth_ratio
            }
        })
    }

    fn canonical_payload(&self) -> serde_json::Value {
        json!({
            "hard_fail_hhi_bits": f64_bits(self.hard_fail_hhi),
            "hard_fail_same_ms_tx_ratio_bits": f64_bits(self.hard_fail_same_ms_tx_ratio),
            "hard_fail_top3_volume_pct_bits": f64_bits(self.hard_fail_top3_volume_pct),
            "max_buy_ratio_bits": f64_bits(self.max_buy_ratio),
            "max_dev_volume_ratio_bits": f64_bits(self.max_dev_volume_ratio),
            "max_funding_source_concentration_bits": f64_bits(self.max_funding_source_concentration),
            "max_hhi_bits": f64_bits(self.max_hhi),
            "max_signer_cross_pool_velocity_bits": f64_bits(self.max_signer_cross_pool_velocity),
            "max_tx_per_signer": self.max_tx_per_signer,
            "min_buy_count": self.min_buy_count,
            "min_buy_ratio_bits": f64_bits(self.min_buy_ratio),
            "min_tx_count": self.min_tx_count,
            "min_unique_signers": self.min_unique_signers,
            "organic_min_tx_count_growth_ratio_bits": f64_bits(self.organic_min_tx_count_growth_ratio),
            "organic_min_unique_signer_growth_ratio_bits": f64_bits(self.organic_min_unique_signer_growth_ratio),
            "reject_on_dev_sell": self.reject_on_dev_sell
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatekeeperV3EvidenceRequirements {
    #[serde(default = "default_required")]
    pub identity: bool,
    #[serde(default = "default_required")]
    pub account_state: bool,
    #[serde(default = "default_required")]
    pub tx_intel: bool,
    #[serde(default = "default_required")]
    pub tx_segments: bool,
    #[serde(default = "default_required")]
    pub checkpoints: bool,
    #[serde(default = "default_required")]
    pub trajectory: bool,
    #[serde(default = "default_required")]
    pub pdd_sequence: bool,
    #[serde(default = "default_required")]
    pub curve: bool,
    #[serde(default = "default_required")]
    pub sybil: bool,
    #[serde(default = "default_required")]
    pub cpv: bool,
    #[serde(default = "default_required")]
    pub fsc: bool,
    #[serde(default = "default_required")]
    pub alpha: bool,
    #[serde(default = "default_required")]
    pub manipulation: bool,
    #[serde(default = "default_required")]
    pub organic_broadening: bool,
    #[serde(default = "default_required")]
    pub manipulation_contradiction: bool,
    #[serde(default)]
    pub execution: bool,
}

impl Default for GatekeeperV3EvidenceRequirements {
    fn default() -> Self {
        Self {
            identity: true,
            account_state: true,
            tx_intel: true,
            tx_segments: true,
            checkpoints: true,
            trajectory: true,
            pdd_sequence: true,
            curve: true,
            sybil: true,
            cpv: true,
            fsc: true,
            alpha: true,
            manipulation: true,
            organic_broadening: true,
            manipulation_contradiction: true,
            execution: false,
        }
    }
}

impl GatekeeperV3EvidenceRequirements {
    pub fn required(&self, group: &str) -> bool {
        match group {
            "identity" => self.identity,
            "account_state" => self.account_state,
            "tx_intel" => self.tx_intel,
            "tx_segments" => self.tx_segments,
            "checkpoints" => self.checkpoints,
            "trajectory" => self.trajectory,
            "pdd_sequence" => self.pdd_sequence,
            "curve" => self.curve,
            "sybil" => self.sybil,
            "cpv" => self.cpv,
            "fsc" => self.fsc,
            "alpha" => self.alpha,
            "manipulation" => self.manipulation,
            "organic_broadening" => self.organic_broadening,
            "manipulation_contradiction" => self.manipulation_contradiction,
            "execution" => self.execution,
            _ => true,
        }
    }

    fn payload(&self) -> serde_json::Value {
        json!({
            "identity": self.identity,
            "account_state": self.account_state,
            "tx_intel": self.tx_intel,
            "tx_segments": self.tx_segments,
            "checkpoints": self.checkpoints,
            "trajectory": self.trajectory,
            "pdd_sequence": self.pdd_sequence,
            "curve": self.curve,
            "sybil": self.sybil,
            "cpv": self.cpv,
            "fsc": self.fsc,
            "alpha": self.alpha,
            "manipulation": self.manipulation,
            "organic_broadening": self.organic_broadening,
            "manipulation_contradiction": self.manipulation_contradiction,
            "execution": self.execution
        })
    }

    fn canonical_payload(&self) -> serde_json::Value {
        self.payload()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatekeeperV3ConfidenceCaps {
    #[serde(default = "default_zero_cap")]
    pub unavailable: f64,
    #[serde(default = "default_zero_cap")]
    pub degraded: f64,
    #[serde(default = "default_zero_cap")]
    pub insufficient_sample: f64,
    #[serde(default = "default_zero_cap")]
    pub stale: f64,
    #[serde(default = "default_zero_cap")]
    pub fallback: f64,
    #[serde(default = "default_zero_cap")]
    pub not_configured: f64,
    #[serde(default = "default_execution_not_run_confidence_cap")]
    pub execution_not_run: f64,
    #[serde(default = "default_zero_cap")]
    pub organic_broadening_insufficient: f64,
    #[serde(default = "default_zero_cap")]
    pub hard_risk: f64,
}

impl Default for GatekeeperV3ConfidenceCaps {
    fn default() -> Self {
        Self {
            unavailable: 0.0,
            degraded: 0.0,
            insufficient_sample: 0.0,
            stale: 0.0,
            fallback: 0.0,
            not_configured: 0.0,
            execution_not_run: default_execution_not_run_confidence_cap(),
            organic_broadening_insufficient: 0.0,
            hard_risk: 0.0,
        }
    }
}

impl GatekeeperV3ConfidenceCaps {
    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, value) in [
            ("unavailable", self.unavailable),
            ("degraded", self.degraded),
            ("insufficient_sample", self.insufficient_sample),
            ("stale", self.stale),
            ("fallback", self.fallback),
            ("not_configured", self.not_configured),
            ("execution_not_run", self.execution_not_run),
            (
                "organic_broadening_insufficient",
                self.organic_broadening_insufficient,
            ),
            ("hard_risk", self.hard_risk),
        ] {
            validate_unit_interval("confidence_caps", name, value)?;
        }
        Ok(())
    }

    fn payload(&self) -> serde_json::Value {
        json!({
            "unavailable": self.unavailable,
            "degraded": self.degraded,
            "insufficient_sample": self.insufficient_sample,
            "stale": self.stale,
            "fallback": self.fallback,
            "not_configured": self.not_configured,
            "execution_not_run": self.execution_not_run,
            "organic_broadening_insufficient": self.organic_broadening_insufficient,
            "hard_risk": self.hard_risk
        })
    }

    fn canonical_payload(&self) -> serde_json::Value {
        json!({
            "unavailable_bits": f64_bits(self.unavailable),
            "degraded_bits": f64_bits(self.degraded),
            "insufficient_sample_bits": f64_bits(self.insufficient_sample),
            "stale_bits": f64_bits(self.stale),
            "fallback_bits": f64_bits(self.fallback),
            "not_configured_bits": f64_bits(self.not_configured),
            "execution_not_run_bits": f64_bits(self.execution_not_run),
            "organic_broadening_insufficient_bits": f64_bits(self.organic_broadening_insufficient),
            "hard_risk_bits": f64_bits(self.hard_risk)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatekeeperV3ComponentWeights {
    #[serde(default = "default_tx_count_weight")]
    pub tx_count: f64,
    #[serde(default = "default_unique_signers_weight")]
    pub unique_signers: f64,
    #[serde(default = "default_buy_count_weight")]
    pub buy_count: f64,
    #[serde(default = "default_buy_ratio_weight")]
    pub buy_ratio: f64,
    #[serde(default = "default_growth_weight")]
    pub growth: f64,
    #[serde(default = "default_max_risk_penalty")]
    pub max_risk_penalty: f64,
}

impl Default for GatekeeperV3ComponentWeights {
    fn default() -> Self {
        Self {
            tx_count: default_tx_count_weight(),
            unique_signers: default_unique_signers_weight(),
            buy_count: default_buy_count_weight(),
            buy_ratio: default_buy_ratio_weight(),
            growth: default_growth_weight(),
            max_risk_penalty: default_max_risk_penalty(),
        }
    }
}

impl GatekeeperV3ComponentWeights {
    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, value) in [
            ("tx_count", self.tx_count),
            ("unique_signers", self.unique_signers),
            ("buy_count", self.buy_count),
            ("buy_ratio", self.buy_ratio),
            ("growth", self.growth),
        ] {
            if !value.is_finite() || value < 0.0 {
                anyhow::bail!(
                    "gatekeeper_v3.component_weights.{name} must be finite and non-negative"
                );
            }
        }
        if self.tx_count + self.unique_signers + self.buy_count + self.buy_ratio + self.growth
            <= 0.0
        {
            anyhow::bail!("gatekeeper_v3.component_weights must have positive total weight");
        }
        validate_unit_interval(
            "component_weights",
            "max_risk_penalty",
            self.max_risk_penalty,
        )?;
        Ok(())
    }

    pub fn total_opportunity_weight(&self) -> f64 {
        self.tx_count + self.unique_signers + self.buy_count + self.buy_ratio + self.growth
    }

    fn payload(&self) -> serde_json::Value {
        json!({
            "tx_count": self.tx_count,
            "unique_signers": self.unique_signers,
            "buy_count": self.buy_count,
            "buy_ratio": self.buy_ratio,
            "growth": self.growth,
            "max_risk_penalty": self.max_risk_penalty
        })
    }

    fn canonical_payload(&self) -> serde_json::Value {
        json!({
            "tx_count_bits": f64_bits(self.tx_count),
            "unique_signers_bits": f64_bits(self.unique_signers),
            "buy_count_bits": f64_bits(self.buy_count),
            "buy_ratio_bits": f64_bits(self.buy_ratio),
            "growth_bits": f64_bits(self.growth),
            "max_risk_penalty_bits": f64_bits(self.max_risk_penalty)
        })
    }
}

fn f64_bits(value: f64) -> String {
    let bits = if value == 0.0 { 0 } else { value.to_bits() };
    format!("{bits:016x}")
}

fn validate_unit_interval(section: &str, name: &str, value: f64) -> anyhow::Result<()> {
    if !value.is_finite() {
        anyhow::bail!("gatekeeper_v3.{section}.{name} must be finite");
    }
    if !(0.0..=1.0).contains(&value) {
        anyhow::bail!("gatekeeper_v3.{section}.{name} must be in [0.0, 1.0]");
    }
    Ok(())
}

const fn default_required() -> bool {
    true
}

const fn default_zero_cap() -> f64 {
    0.0
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

const fn default_tx_count_weight() -> f64 {
    0.25
}

const fn default_unique_signers_weight() -> f64 {
    0.25
}

const fn default_buy_count_weight() -> f64 {
    0.20
}

const fn default_buy_ratio_weight() -> f64 {
    0.15
}

const fn default_growth_weight() -> f64 {
    0.15
}

const fn default_max_risk_penalty() -> f64 {
    0.85
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gatekeeper_v3_config_defaults_are_shadow_disabled() {
        let config = GatekeeperV3Config::default();
        assert!(!config.enabled);
        assert!(!config.shadow_emit_enabled);
        assert!(!config.replay_payload_enabled);
        assert_eq!(config.policy_version, 1);
        assert_eq!(config.materialization_version, 1);
        assert_eq!(config.early_window_ms, 2_000);
        assert!(!config.promotion.enabled);
        assert!(!config.evidence_requirements.execution);
        assert_eq!(config.confidence_caps.execution_not_run, 0.80);
        assert_eq!(config.component_weights.max_risk_penalty, 0.85);
    }

    #[test]
    fn gatekeeper_v3_config_hash_is_stable_and_sensitive() {
        let config = GatekeeperV3Config::default();
        assert_eq!(
            config.v3_policy_config_hash(),
            config.v3_policy_config_hash()
        );

        let mut changed = config.clone();
        changed.early_window_ms += 1;
        assert_ne!(
            config.v3_policy_config_hash(),
            changed.v3_policy_config_hash()
        );

        let mut changed = config.clone();
        changed.normal.min_tx_count += 1;
        assert_ne!(
            config.v3_policy_config_hash(),
            changed.v3_policy_config_hash()
        );

        let mut changed = config.clone();
        changed.component_weights.tx_count += 0.01;
        assert_ne!(
            config.v3_policy_config_hash(),
            changed.v3_policy_config_hash()
        );
    }

    #[test]
    fn gatekeeper_v3_config_loads_without_replay_payload_field() {
        let config: GatekeeperV3Config = toml::from_str(
            r#"
enabled = false
shadow_emit_enabled = true
policy_version = 1
materialization_version = 1
"#,
        )
        .unwrap();

        assert!(config.shadow_emit_enabled);
        assert!(!config.replay_payload_enabled);
        assert_eq!(config.policy_version, 1);
        assert_eq!(config.materialization_version, 1);
    }

    #[test]
    fn replay_payload_flag_does_not_change_policy_hash() {
        let config = GatekeeperV3Config::default();
        let mut changed = config.clone();
        changed.replay_payload_enabled = true;

        assert_eq!(
            config.v3_policy_config_hash(),
            changed.v3_policy_config_hash()
        );
    }

    #[test]
    fn gatekeeper_v3_config_rejects_invalid_thresholds() {
        let mut config = GatekeeperV3Config::default();
        config.confidence_caps.execution_not_run = 1.1;
        assert!(config.validate().is_err());

        let mut config = GatekeeperV3Config::default();
        config.component_weights.tx_count = 0.0;
        config.component_weights.unique_signers = 0.0;
        config.component_weights.buy_count = 0.0;
        config.component_weights.buy_ratio = 0.0;
        config.component_weights.growth = 0.0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn gatekeeper_v3_profile_selector_uses_early_normal_extended_context() {
        let config = GatekeeperV3Config::default();

        assert_eq!(config.profile_name_for_context(false, 0), "early");
        assert_eq!(config.profile_name_for_context(false, 1_999), "early");
        assert_eq!(config.profile_name_for_context(false, 2_000), "normal");
        assert_eq!(config.profile_name_for_context(true, 0), "extended");
    }
}
